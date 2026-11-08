#!/bin/sh
# Offline checks of the full-system bundle's names, its settings and the commands its guide
# gives (ADR 0147). Needs Docker Compose v2.20+ and python3; nothing is started or pulled.
#
#   sh deploy/compose/check-isolation.sh
#
# Renders the bundle twice under two project names, from a copy of this folder whose .env is
# bundle.env.example, and checks that Core's own networks and containers follow the project
# rather than fixed names, so two installs can run side by side. Console's and Audio's files are
# vendored verbatim (SOURCES.json) and still fix some names; those are printed as KNOWN LIMIT,
# not hidden, until a new pin changes them.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0
ok() { echo "ok   $*"; }
bad() { echo "FAIL $*"; fail=1; }

# Names that Console's and Audio's vendored files fix (their container_name and macvlan network).
vendored='endora-console endora-audio endora-audio-spotify endora-audio-senders endora-audio-airplay endora-audio-airplay-lan'

cp -R "$here/." "$work/bundle"
cp "$work/bundle/bundle.env.example" "$work/bundle/.env"
cp -R "$here/core" "$work/core"
cp "$work/core/core.env.example" "$work/core/.env"

# render DIR PROJECT OUT: the model as JSON, with the workers profile on so the runner is in it.
render() { (cd "$1" && docker compose --profile workers -p "$2" config --format json > "$3" 2> "$3.err"); }

echo "== The example renders with no edit"
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
if render "$work/core" endora-core "$work/core.json"; then
  ok "Core alone renders from core.env.example"
else
  bad "core/core.env.example does not render: $(head -1 "$work/core.json.err")"
fi

echo "== Two installs on one host"
render "$work/bundle" endora-b "$work/b.json" || { bad "second project does not render"; exit 1; }

# names JSON: the network and container names a rendered model uses, and the runner's team network.
names() {
  python3 - "$1" <<'PY'
import json, sys
j = json.load(open(sys.argv[1]))
nets = {k: v.get("name", k) for k, v in j.get("networks", {}).items()}
cons = [s["container_name"] for s in j.get("services", {}).values() if "container_name" in s]
runner = j.get("services", {}).get("runner", {}).get("environment", {})
print(json.dumps({"project": j["name"], "team": nets.get("team"), "nets": sorted(set(nets.values())),
                  "cons": sorted(set(cons)), "runner_team": runner.get("TEAM_NETWORK")}))
PY
}
names "$work/a.json" > "$work/a.names"
names "$work/b.json" > "$work/b.names"
names "$work/core.json" > "$work/core.names"

for f in a b core; do
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

# Anything both installs share, other than the vendored names, is a collision.
python3 - "$work/a.names" "$work/b.names" $vendored <<'PY' > "$work/shared"
import json, sys
a = json.load(open(sys.argv[1])); b = json.load(open(sys.argv[2])); vendored = set(sys.argv[3:])
shared = (set(a["nets"]) | set(a["cons"])) & (set(b["nets"]) | set(b["cons"]))
for name in sorted(shared):
    print(("known " if name in vendored else "collision ") + name)
PY
grep '^collision ' "$work/shared" > "$work/shared.collision" || true
grep '^known ' "$work/shared" > "$work/shared.known" || true
if [ -s "$work/shared.collision" ]; then
  while IFS= read -r line; do bad "two installs share ${line#collision }"; done < "$work/shared.collision"
else
  ok "two installs share no network or container name that Core or the bundle owns"
fi
while IFS= read -r line; do
  echo "KNOWN LIMIT: ${line#known } is fixed by Console's or Audio's vendored file; a second install on one host needs a new pin"
done < "$work/shared.known"

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

echo
if [ "$fail" -ne 0 ]; then
  echo "isolation check failed"
  exit 1
fi
echo "isolation check passed (KNOWN LIMIT lines, if any, are vendored names, not failures)"
