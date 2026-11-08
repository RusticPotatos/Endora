#!/bin/sh
# Offline checks of the full-system bundle's names, its settings and the commands its guide
# gives (ADR 0147). Needs Docker Compose v2.20+ and python3; nothing is started or pulled.
#
#   sh deploy/compose/check-isolation.sh
#
# Renders the bundle, Core alone, and a second Core from core/second.env.example, each under its
# own project name, from copies of this folder. For every model it compares the names a running
# install would own: container names, network names, volume names and published host ports.
#
# The supported concurrent install is a second Core beside the bundle or beside Core alone: it
# must share none of those names. Two full bundles on one host are not supported: Console's and
# Audio's vendored files fix container names and Audio runs host-network services. Those shared
# names are printed as KNOWN LIMIT, not hidden, and the guide must say one bundle per host.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0
ok() { echo "ok   $*"; }
bad() { echo "FAIL $*"; fail=1; }

if [ ! -f "$here/core/second.env.example" ]; then
  bad "core/second.env.example is missing: the documented second Core (ADR 0147) has no settings"
  echo "isolation check failed"
  exit 1
fi

cp -R "$here/." "$work/bundle"
cp "$work/bundle/bundle.env.example" "$work/bundle/.env"
cp -R "$here/core" "$work/core"
cp "$work/core/core.env.example" "$work/core/.env"
cp -R "$here/core" "$work/second"
cp "$here/core/second.env.example" "$work/second/.env"

# render DIR PROJECT OUT: the model as JSON, with the workers and airplay profiles on, so the runner
# and the AirPlay door (and their names) are in it.
render() { (cd "$1" && docker compose --profile workers --profile airplay -p "$2" config --format json > "$3" 2> "$3.err"); }

echo "== The examples render with no edit"
if ! render "$work/bundle" endora-a "$work/a.json"; then
  bad "bundle.env.example does not render: $(head -1 "$work/a.json.err")"
  exit 1
fi
ok "the bundle renders from bundle.env.example with no edit"
if ! grep -Eq '^ENDORA_CORE_VERSION=(sha-[0-9a-f]{40}|[0-9]+\.[0-9]+\.[0-9]+)$' "$here/bundle.env.example"; then
  bad "bundle.env.example: ENDORA_CORE_VERSION is not a published tag (sha-<40 hex> or X.Y.Z)"
else
  ok "bundle.env.example: ENDORA_CORE_VERSION names a published tag"
fi
if ! grep -Eq '^ENDORA_CORE_VERSION=(sha-[0-9a-f]{40}|[0-9]+\.[0-9]+\.[0-9]+)$' "$here/core/core.env.example"; then
  bad "core/core.env.example: ENDORA_CORE_VERSION is not a published tag (sha-<40 hex> or X.Y.Z)"
else
  ok "core/core.env.example: ENDORA_CORE_VERSION names a published tag"
fi
if ! grep -Eq '^ENDORA_CORE_VERSION=(sha-[0-9a-f]{40}|[0-9]+\.[0-9]+\.[0-9]+)$' "$here/core/second.env.example"; then
  bad "core/second.env.example: ENDORA_CORE_VERSION is not a published tag (sha-<40 hex> or X.Y.Z)"
else
  ok "core/second.env.example: ENDORA_CORE_VERSION names a published tag"
fi
if render "$work/core" endora-core "$work/core.json"; then
  ok "Core alone renders from core.env.example"
else
  bad "core/core.env.example does not render: $(head -1 "$work/core.json.err")"
fi
if render "$work/second" endora-core-second "$work/second.json"; then
  ok "a second Core renders from core/second.env.example"
else
  bad "core/second.env.example does not render: $(head -1 "$work/second.json.err")"
  exit 1
fi

# names JSON: every name a rendered model owns that another install could collide with.
names() {
  python3 - "$1" <<'PY'
import json, sys
j = json.load(open(sys.argv[1]))
svcs = j.get("services", {})
nets = {k: v.get("name", k) for k, v in j.get("networks", {}).items()}
vols = {k: v.get("name", k) for k, v in j.get("volumes", {}).items()}
cons = [s["container_name"] for s in svcs.values() if "container_name" in s]
ports = [str(p["published"]) for s in svcs.values() for p in s.get("ports", []) if p.get("published")]
runner = svcs.get("runner", {}).get("environment", {})
# hostnet: services on the host's network. Compose publishes none of their ports, so their own
# ports (Audio's senders: 3701-3704/tcp, 319-320/udp) are named by the guide, not read here.
hostnet = sorted(n for n, s in svcs.items() if s.get("network_mode") == "host")
print(json.dumps({"project": j["name"], "team": nets.get("team"), "runner_team": runner.get("TEAM_NETWORK"),
                  "nets": sorted(set(nets.values())), "cons": sorted(set(cons)),
                  "vols": sorted(set(vols.values())), "ports": sorted(set(ports)), "hostnet": hostnet}))
PY
}
names "$work/a.json" > "$work/a.names"
names "$work/core.json" > "$work/core.names"
names "$work/second.json" > "$work/second.names"

# shared A B: one "kind name" line for each name both models own (networks, containers, volumes, ports).
shared() {
  python3 - "$1" "$2" <<'PY'
import json, sys
a = json.load(open(sys.argv[1])); b = json.load(open(sys.argv[2]))
for kind in ("nets", "cons", "vols", "ports", "hostnet"):
    for name in sorted(set(a[kind]) & set(b[kind])):
        print(kind, name)
PY
}

echo "== Each install's team network follows its project"
for f in a core second; do
  # The team network is the project's own (`<project>_team`), and the runner is told that name.
  verdict=$(python3 - "$work/$f.names" <<'PY'
import json, sys
n = json.load(open(sys.argv[1]))
want = n["project"] + "_team"
if n["team"] != want:
    print("bad team network %r, want %r" % (n["team"], want))
elif n["runner_team"] != n["team"]:
    print("bad runner TEAM_NETWORK %r, want %r" % (n["runner_team"], n["team"]))
else:
    print("ok")
PY
)
  case $verdict in
    ok) ok "$f: the team network follows the project, and the runner uses it" ;;
    *) bad "$f: $verdict" ;;
  esac
done

echo "== A second Core beside the bundle and beside Core alone"
for other in a core; do
  shared "$work/second.names" "$work/$other.names" > "$work/second.$other"
  if [ -s "$work/second.$other" ]; then
    while IFS= read -r line; do bad "second Core shares ${line} with $other"; done < "$work/second.$other"
  else
    ok "a second Core shares no network, container, volume or host port with $other"
  fi
done

echo "== Default volume names are the ones existing installs use (upgrades keep the data)"
historic='endora-core-data endora-core-hq-data endora-core-hq-token endora-core-hq-runner-token'
for f in a core; do
  got=$(python3 -c "import json,sys; v=json.load(open(sys.argv[1]))['vols']; want=sys.argv[2].split(); print('ok' if all(x in v for x in want) else 'missing ' + ' '.join(x for x in want if x not in v))" "$work/$f.names" "$historic")
  case $got in
    ok) ok "$f: Core's four volumes keep their names" ;;
    *) bad "$f: $got" ;;
  esac
done

echo "== Two full bundles on one host (not supported: the shared names are listed)"
render "$work/bundle" endora-b "$work/b.json" || { bad "second bundle project does not render"; exit 1; }
names "$work/b.json" > "$work/b.names"
shared "$work/a.names" "$work/b.names" > "$work/bundles"
while IFS= read -r line; do
  case $line in
    "hostnet "*) echo "KNOWN LIMIT: ${line#hostnet } uses the host's network, so its ports belong to the host; one bundle per host runs it (docs/install.md)" ;;
    *) echo "KNOWN LIMIT: ${line} is shared by two full bundles; one bundle per host (docs/install.md)" ;;
  esac
done < "$work/bundles"

echo "== The check sees every fixed container name the vendored files set"
grep -h 'container_name:' "$here/core/compose.yaml" "$here/console/compose.yaml" "$here/audio/compose.yaml" \
  | sed 's/^[[:space:]]*container_name:[[:space:]]*//; s/[[:space:]]*$//' | sort -u > "$work/fixed-names"
python3 - "$work/a.json" "$work/fixed-names" > "$work/unseen" <<'PY'
import json, sys
j = json.load(open(sys.argv[1]))
seen = {s["container_name"] for s in j.get("services", {}).values() if "container_name" in s}
for name in open(sys.argv[2]).read().split():
    if name not in seen:
        print(name)
PY
if [ -s "$work/unseen" ]; then
  while IFS= read -r n; do bad "container name $n is fixed in a vendored file, but the check does not render it"; done < "$work/unseen"
else
  ok "every fixed container name in the vendored files is rendered and checked"
fi

echo "== Two full bundles name every host-network service (compose shows none of its ports)"
python3 - "$work/a.json" > "$work/hostnet-expected" <<'PY'
import json, sys
j = json.load(open(sys.argv[1]))
for n, s in j.get("services", {}).items():
    if s.get("network_mode") == "host":
        print(n)
PY
if [ ! -s "$work/hostnet-expected" ]; then
  bad "no host-network service is rendered, so the host's ports cannot be checked (Audio's senders expected)"
fi
while IFS= read -r svc; do
  if grep -qx "hostnet $svc" "$work/bundles"; then
    ok "$svc uses the host's network, and the two-bundle case names it"
  else
    bad "$svc uses the host's network, and the two-bundle case does not name it"
  fi
done < "$work/hostnet-expected"

echo "== A second full bundle with every documented override shares only fixed names"
overrides='CONSOLE_PORT=8797
ENDORA_CORE_PORT=8796
ENDORA_AUDIO_PORT=8792
ENDORA_CORE_DATA_VOLUME=b2-core-data
ENDORA_CORE_HQ_DATA_VOLUME=b2-core-hq-data
ENDORA_CORE_HQ_TOKEN_VOLUME=b2-core-hq-token
ENDORA_CORE_HQ_RUNNER_TOKEN_VOLUME=b2-core-hq-runner-token
ENDORA_AUDIO_DATA_VOLUME=b2-audio-data
ENDORA_AUDIO_TOKEN_VOLUME=b2-audio-token
ENDORA_AUDIO_PIPES_VOLUME=b2-audio-pipes
ENDORA_AUDIO_SECRET_VOLUME=b2-audio-secret
ENDORA_AUDIO_SENDERS_DATA_VOLUME=b2-audio-senders-data
ENDORA_AUDIO_SPOTIFY_LOGIN_VOLUME=b2-audio-spotify-login
SOUND_AIRPLAY2_NETWORK=b2-audio-airplay-lan'
for var in $(printf '%s\n' "$overrides" | cut -d= -f1); do
  grep -q "\${$var:-" "$here/core/compose.yaml" "$here/console/compose.yaml" "$here/audio/compose.yaml" \
    || bad "override $var is not read by any vendored compose file"
done
# second_check DIR OMIT PROJECT: DIR is a copy of this folder whose .env sets every override except
# OMIT (empty: all). Prints each name it shares with the first bundle that is not a fixed one.
second_check() {
  mkdir -p "$1"
  cp -R "$here/." "$1"
  cp "$here/bundle.env.example" "$1/.env"
  printf '%s\n' "$overrides" | grep -v "^$2=" >> "$1/.env"
  if ! render "$1" "$3" "$1.json"; then
    echo "RENDER-FAILED: $(head -1 "$1.json.err")"
    return 0
  fi
  names "$1.json" > "$1.names"
  shared "$work/a.names" "$1.names" | grep -v -E '^(cons|hostnet) ' || true
}
residual=$(second_check "$work/b2" "" endora-b2)
if [ -n "$residual" ]; then
  while IFS= read -r line; do bad "a second full bundle with every override shares ${line} with the first"; done <<EOF
$residual
EOF
else
  ok "a second full bundle with every override shares only fixed container names and host networks"
fi
# Negative controls: one missed override must be caught, or the test proves nothing.
for omit in ENDORA_CORE_HQ_TOKEN_VOLUME ENDORA_AUDIO_PORT; do
  if [ -n "$(second_check "$work/neg-$omit" "$omit" endora-neg)" ]; then
    ok "negative control: a second bundle without $omit is caught"
  else
    bad "negative control: a second bundle without $omit was not caught"
  fi
done

echo "== The guide"
docs="$here/../../docs/install.md"
# A piped `docker compose exec` needs -T (no terminal); every piped one in the guide has it.
if grep -n '|[[:space:]]*docker compose exec' "$docs" | grep -v -- ' -T ' > "$work/piped"; then
  while IFS= read -r line; do bad "piped exec without -T in the guide: $line"; done < "$work/piped"
else
  ok "every piped exec in the guide uses -T"
fi
if grep -Eq "printf .*\| *docker compose exec -T node endora-core setup" "$docs"; then
  ok "the guide shows scripted setup from stdin"
else
  bad "the guide does not show scripted setup from stdin (printf ... | docker compose exec -T node endora-core setup)"
fi
# A tarball written by a root container must be given back to the user who runs the guide.
if grep 'tar czf' "$docs" | grep -qv 'chown'; then
  bad "the guide writes a backup tarball as root: give it to the user with chown"
else
  ok "every backup tarball in the guide is handed back to the user"
fi
# The team network is named by the project, never by the old fixed name. The one allowed mention
# is the guide's note on moving a Core started before this change ("was `endora-core-team`").
if grep -rn --exclude=check-isolation.sh 'endora-core-team' "$here" "$docs" | grep -v 'was `endora-core-team`' > "$work/fixed" 2>/dev/null; then
  while IFS= read -r line; do bad "fixed network name still referenced: $line"; done < "$work/fixed"
else
  ok "no reference to the old fixed network name remains"
fi
# The second install is the documented one: the guide names its settings file and says one full
# bundle per host, so the unsupported case is never presented as working.
if grep -q 'core/second.env.example' "$docs"; then
  ok "the guide names the second Core's settings (core/second.env.example)"
else
  bad "the guide does not name core/second.env.example for a second install"
fi
if grep -q 'one full bundle per host' "$docs"; then
  ok "the guide says one full bundle per host"
else
  bad "the guide does not say one full bundle per host"
fi

echo
if [ "$fail" -ne 0 ]; then
  echo "isolation check failed"
  exit 1
fi
echo "isolation check passed (KNOWN LIMIT lines are unsupported shared names, not passes of them)"
