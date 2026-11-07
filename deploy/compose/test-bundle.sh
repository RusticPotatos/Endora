#!/bin/sh
# The full-system bundle's test (ADR 0147): brings compose.yaml up with a stub model, checks
# every product and the sign-in through `endora-core setup`, then takes it all down again.
#
#   sh deploy/compose/test-bundle.sh
#
# Needs Docker with Compose v2.20+, curl and python3, and a Core image tagged as ci/ci.env says
# (`docker build -t ghcr.io/rusticpotatos/endora-node:ci .` from the repository's root). It runs
# in a copy of this folder with bundle.env.example plus ci/ci.env's overrides as its .env, so your own .env is never read or
# touched, and its volumes are named endora-ci-*, so `down -v` removes only the test's. Run it
# on a host with no Endora running (CI's plain ubuntu runner): it uses ports 8786, 8787, 8791
# and Audio's host-network ports. TEST_BUNDLE_KEEP=1 leaves the stack up for a look.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
cp -R "$here/." "$work/"
# A user's .env (bundle.env.example) with the test's overrides from ci/ci.env: a key set in
# ci.env replaces the example's line, so every pin is read from the example itself.
awk -F= '/^[A-Za-z_][A-Za-z_0-9]*=/ { if (FILENAME ~ /ci\.env$/) over[$1] = $0; else { order[++n] = $1; line[$1] = $0 } }
  END { for (i = 1; i <= n; i++) { k = order[i]; print (k in over) ? over[k] : line[k]; delete over[k] }
        for (k in over) print over[k] }' "$work/bundle.env.example" "$work/ci/ci.env" > "$work/.env"
cd "$work"

compose() { docker compose -f compose.yaml -f ci/compose.stub-model.yaml "$@"; }
console=http://127.0.0.1:8787
origin="Origin: $console"
failures=0

finish() {
  status=$?
  if [ "$failures" -ne 0 ] || [ "$status" -ne 0 ]; then
    compose ps -a || true
    compose logs --no-color --tail 80 || true
  fi
  if [ "${TEST_BUNDLE_KEEP:-0}" != 1 ]; then
    compose down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  rm -rf "$work"
}
trap finish EXIT

passed="$work/.passed"
: > "$passed"
pass() { printf 'ok   %s\n' "$1"; printf '%s\n' "$1" >> "$passed"; }
fail() { printf 'FAIL %s\n' "$1"; failures=$((failures + 1)); }
# check NAME COMMAND...: one named check, its command's success is the verdict.
check() { name=$1; shift; if "$@"; then pass "$name"; else fail "$name"; fi; }

# json URL EXPR [CURL ARGS...]: fetches URL and prints the Python expression EXPR of its JSON `j`.
json() {
  url=$1; expr=$2; shift 2
  curl -fsS --max-time 10 "$@" "$url" | python3 -c "import json,sys; j=json.load(sys.stdin); print($expr)"
}
# status URL [CURL ARGS...]: the HTTP status alone.
status() { url=$1; shift; curl -s -o /dev/null -w '%{http_code}' --max-time 10 "$@" "$url"; }

echo "== The pins"
check "every vendored compose file matches its pin" sh check-sources.sh

echo "== up"
compose config --quiet
compose up -d --wait --wait-timeout 300

echo "== Core"
check "Core is healthy and says API 1.0" \
  test "$(json http://127.0.0.1:8786/health "j['status'], j['api']")" = "ok 1.0"
check "HQ is healthy inside the bundle" \
  compose exec -T hq python3 -c "import json,urllib.request as u;assert json.load(u.urlopen('http://127.0.0.1:8790/health'))['ok']"
check "Core is not published beyond the host's loopback" \
  test "$(compose port node 8787)" = "127.0.0.1:8786"

echo "== Console in front of Core"
check "Console serves its page" sh -c "curl -fsS --max-time 10 $console/ | grep -qi '<html'"
check "Console reaches a compatible Core" \
  test "$(json $console/console/health "j['status'], j['core']['reachable'], j['core']['compatible'], j['core']['api']")" = "ok True True 1.0"
check "Console proxies the contract's /health to Core" \
  test "$(json $console/health "j['service'] != '', j['api']")" = "True 1.0"
check "Console refuses a route outside the contract" test "$(status $console/v1/runner/next)" = 404
check "Console refuses HQ's own routes" test "$(status $console/v1/hq/desk/1)" = 404
check "Core refuses an unsigned write that Console passed on" \
  test "$(status $console/v1/session/sign-out -X POST -H "$origin" -H 'content-type: application/json' -d '{}')" = 401
check "Console refuses a write from another origin" \
  test "$(status $console/v1/session/sign-out -X POST -H 'Origin: http://elsewhere.example.com' -H 'content-type: application/json' -d '{}')" = 403

echo "== Audio"
check "Audio is healthy on its own port" test "$(json http://127.0.0.1:8791/health "j['ok']")" = True
check "Console shows Audio's status from Audio's own check" \
  test "$(json $console/console/audio "j['configured'], j['ok']")" = "True True"

echo "== First run: endora-core setup, then sign-in through Console"
password="a long placeholder password for the bundle test"
made=$(printf '%s\n' "$password" | compose exec -T node endora-core setup)
key=$(printf '%s\n' "$made" | sed -n 's/^or type the key: //p')
check "setup made the owner and printed an authenticator key" test -n "$key"
code=$(python3 - "$key" <<'PY'
import base64, hashlib, hmac, struct, sys, time
key = sys.argv[1]
secret = base64.b32decode(key + "=" * (-len(key) % 8))
mac = hmac.new(secret, struct.pack(">Q", int(time.time()) // 30), hashlib.sha1).digest()
at = mac[-1] & 15
print("%06d" % ((struct.unpack(">I", mac[at:at + 4])[0] & 0x7FFFFFFF) % 1000000))
PY
)
body=$(printf '{"password":"%s","code":"%s"}' "$password" "$code")
token=$(json $console/v1/session "j['token']" -X POST -H "$origin" -H 'content-type: application/json' -d "$body" || true)
check "the owner signs in through Console with the password and a code" test "${#token}" -ge 32
check "the session is Core's, full scope" \
  test "$(json $console/v1/session/current "j['scope']" -H "Authorization: Bearer $token")" = full
check "a wrong code is refused by Core" \
  test "$(status $console/v1/session -X POST -H "$origin" -H 'content-type: application/json' -d "$(printf '{"password":"%s","code":"000000"}' "$password")")" = 400
check "setup refuses to make a second owner" \
  sh -c "! printf 'another long placeholder password\n' | docker compose -f compose.yaml -f ci/compose.stub-model.yaml exec -T node endora-core setup >/dev/null 2>&1"

echo "== The model, through Core"
check "a signed read through Console reaches Core's model view" \
  test "$(status $console/v1/ai -H "Authorization: Bearer $token")" = 200
check "Core asked the stub model host" sh -c "docker compose -f compose.yaml -f ci/compose.stub-model.yaml logs stub-model | grep -q 'stub-model: GET /api/ps'"

echo "== Sign-out"
check "signing out through Console ends the session on Core" \
  sh -c "curl -fsS -o /dev/null -X POST -H '$origin' -H 'Authorization: Bearer $token' -H 'content-type: application/json' -d '{}' $console/v1/session/sign-out && test \"\$(curl -s -o /dev/null -w '%{http_code}' -H 'Authorization: Bearer $token' http://127.0.0.1:8786/v1/session/current)\" = 401"

echo "== Every scenario step was checked"
# Each Then/And of the feature is a check above; one that did not pass (or never ran) fails.
steps=$(sed -nE 's/^ *(Then|And) (.*)$/\2/p' features/full-system-bundle.feature | grep -v '^a stub model host' || true)
total=0
printf '%s\n' "$steps" > "$work/.steps"
while IFS= read -r step; do
  [ -n "$step" ] || continue
  total=$((total + 1))
  grep -qxF "$step" "$passed" || fail "scenario step not passed: $step"
done < "$work/.steps"
echo "scenario steps checked: $total"

echo
if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed"
  exit 1
fi
echo "every check passed"
