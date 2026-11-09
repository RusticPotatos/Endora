#!/bin/sh
# Offline checks of the full-system bundle's names, its settings and the commands its guide
# gives (ADR 0147). Needs Docker Compose v2.20+ and python3; nothing is started or pulled.
#
#   sh deploy/compose/check-isolation.sh
#   sh deploy/compose/check-isolation.sh --self-test   (offline; needs only sh and grep)
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

# old_name_refs OWN PATH...: prints "file:line:text" for every mention of the old fixed team network
# name under PATH..., except lines of the file OWN (this script names it to look for it) and the
# guide's one allowed mention. Returns 2 when grep itself fails: an empty result then means "could
# not look", never "none found". Only `grep -rn` is used, which GNU and BusyBox grep both take;
# BusyBox rejects --exclude, and that error, read as "no matches", once made this check pass empty.
old_name_refs() {
  own=$1; shift
  rc=0
  grep -rn 'endora-core-team' "$@" > "$work/refs.out" 2> "$work/refs.err" || rc=$?
  if [ "$rc" -gt 1 ]; then
    echo "grep failed (exit $rc): $(head -1 "$work/refs.err")" >&2
    return 2
  fi
  [ "$rc" -eq 1 ] && return 0
  while IFS= read -r line; do
    case $line in
      "$own":*) continue ;;
      *'was `endora-core-team`'*) continue ;;
    esac
    printf '%s\n' "$line"
  done < "$work/refs.out"
  return 0
}

if [ "${1:-}" = --self-test ]; then
  t="$work/self"; mkdir -p "$t/tree" "$t/clean" "$t/shim"
  printf 'network: endora-core-team\n' > "$t/tree/leak.txt"
  printf 'network: endora-core-team\n' > "$t/tree/own.sh"
  printf 'moving a Core that was `endora-core-team`\n' > "$t/tree/guide.md"
  printf 'nothing here\n' > "$t/clean/note.txt"
  real=$(command -v grep)
  # BusyBox grep rejects --exclude (exit 2): a grep that does the same.
  printf '#!/bin/sh\nfor a in "$@"; do case $a in --exclude*|--include*) echo "grep: unrecognized option: ${a#--}" >&2; exit 2;; esac; done\nexec %s "$@"\n' "$real" > "$t/shim/grep"
  mkdir -p "$t/broken"
  printf '#!/bin/sh\necho "grep: cannot run" >&2\nexit 2\n' > "$t/broken/grep"
  chmod +x "$t/shim/grep" "$t/broken/grep"
  set +e   # each assertion reports its own status
  st_fail=0
  st() { if [ "$2" -eq 0 ]; then echo "ok   self-test: $1"; else echo "FAIL self-test: $1"; st_fail=1; fi; }

  rc=0; out=$(old_name_refs "$t/tree/own.sh" "$t/tree" 2>/dev/null) || rc=$?
  [ "$rc" -eq 0 ] && [ "$(printf '%s\n' "$out" | wc -l)" -eq 1 ] && case $out in "$t/tree/leak.txt:"*) true ;; *) false ;; esac
  st "a planted reference is reported; the script's own file and the guide's allowed mention are not" $?

  rc=0; out=$(PATH="$t/shim:$PATH" old_name_refs "$t/tree/own.sh" "$t/tree" 2>/dev/null) || rc=$?
  [ "$rc" -ne 0 ] || case $out in "$t/tree/leak.txt:"*) true ;; *) false ;; esac
  st "negative control: with a grep that rejects --exclude (BusyBox), a planted reference is still caught or the search fails" $?

  rc=0; out=$(PATH="$t/broken:$PATH" old_name_refs "$t/tree/own.sh" "$t/clean" 2>/dev/null) || rc=$?
  [ "$rc" -ne 0 ]
  st "negative control: a grep that errors makes the search fail, never read as 'no references'" $?

  rc=0; out=$(old_name_refs "$t/tree/own.sh" "$t/clean" 2>/dev/null) || rc=$?
  [ "$rc" -eq 0 ] && [ -z "$out" ]
  st "a clean tree passes with no references" $?
  exit "$st_fail"
fi

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

echo "== Core's fenced services: signals, start order and safe defaults"
# core_facts MODEL: what the cutover of 9 Nov 2026 found. Node, HQ and the runner run their program as
# PID 1 with no signal handler, so `docker stop` waited out the grace period and killed them (exit
# 137): an init process (`init: true`) forwards SIGTERM. The node asked HQ for its state before HQ
# was up (three "HQ is not answering" lines): it starts when HQ is healthy. The deploy key's mount
# defaults to an empty volume, so nothing is bind-mounted unless the owner names a folder, and the
# team listener (8788) is published only when the owner adds it (docs/install.md).
core_facts() {
  python3 - "$1" "${2:-}" <<'PY'
import json, sys
j = json.load(open(sys.argv[1])); want_key = sys.argv[2]
svcs = j["services"]
out = []
for name in ("node", "hq", "runner"):
    s = svcs[name]
    out.append("init:%s:%s" % (name, s.get("init") is True))
    out.append("fenced:%s:%s" % (name, s.get("cap_drop") == ["ALL"] and "no-new-privileges:true" in s.get("security_opt", [])))
dep = svcs["node"].get("depends_on", {}).get("hq", {})
out.append("node-after-hq:%s" % (dep.get("condition") == "service_healthy"))
out.append("hq-read-only:%s" % (svcs["hq"].get("read_only") is True))
published = [str(p.get("target")) for p in svcs["node"].get("ports", [])]
out.append("team-unpublished:%s" % ("8788" not in published))
env = svcs["node"]["environment"]
out.append("key-file:%s" % (env.get("ENDORA_GIT_PUSH_KEY_FILE") == "/git-push-key/key"))
mount = [v for v in svcs["node"].get("volumes", []) if v.get("target") == "/git-push-key"]
if len(mount) != 1:
    out.append("key-mount:False")
else:
    m = mount[0]
    if want_key:
        out.append("key-mount:%s" % (m.get("type") == "bind" and m.get("source") == want_key and m.get("read_only") is True))
    else:
        out.append("key-mount:%s" % (m.get("type") == "volume" and m.get("read_only") is True
                                     and j["volumes"][m["source"]].get("name", "").endswith("endora-no-git-key")))
print("\n".join(out))
PY
}
for f in a core second; do
  core_facts "$work/$f.json" > "$work/$f.facts" 2> "$work/$f.facts.err" || { bad "$f: could not read the model: $(tail -1 "$work/$f.facts.err")"; continue; }
  for fact in init:node init:hq init:runner fenced:node fenced:hq fenced:runner node-after-hq hq-read-only team-unpublished key-file key-mount; do
    case $fact in
      init:*)         what="${fact#init:} runs under an init process (it forwards SIGTERM, so docker stop is clean)" ;;
      fenced:*)       what="${fact#fenced:} keeps cap_drop ALL and no-new-privileges beside init" ;;
      node-after-hq)  what="the node starts when HQ is healthy" ;;
      hq-read-only)   what="HQ's root stays read-only beside init" ;;
      team-unpublished) what="the team listener (8788) is not published by default" ;;
      key-file)       what="the node is told where the deploy key would be" ;;
      key-mount)      what="the deploy key's mount defaults to an empty read-only volume, no host path" ;;
    esac
    if grep -qx "$fact:True" "$work/$f.facts"; then ok "$f: $what"; else bad "$f: $what"; fi
  done
done
# The deploy key's folder is a variable; an absolute path becomes a read-only bind of that folder.
mkdir -p "$work/keyed"
cp -R "$here/core/." "$work/keyed"
cp "$here/core/core.env.example" "$work/keyed/.env"
printf 'ENDORA_GIT_PUSH_KEY_DIR=/srv/example/git-push-key\n' >> "$work/keyed/.env"
if render "$work/keyed" endora-core "$work/keyed.json" && core_facts "$work/keyed.json" /srv/example/git-push-key | grep -qx 'key-mount:True'; then
  ok "ENDORA_GIT_PUSH_KEY_DIR names the folder holding the deploy key (mounted read-only)"
else
  bad "ENDORA_GIT_PUSH_KEY_DIR does not mount the named folder read-only at /git-push-key"
fi
# The runtime combination, read-only root plus every capability dropped plus an init, starts. Skipped
# (said so) when alpine:3.21, the image the guide's own backup commands use, is not on the host.
img=alpine:3.21
if docker image inspect "$img" >/dev/null 2>&1; then
  if docker run --rm --init --read-only --cap-drop ALL --security-opt no-new-privileges:true --pull never "$img" true >/dev/null 2>&1; then
    ok "an init process runs under read_only, cap_drop ALL and no-new-privileges (Docker accepted it)"
  else
    bad "init conflicts with read_only, cap_drop ALL or no-new-privileges on this Docker"
  fi
else
  echo "skip init runtime check: $img is not on this host (nothing is pulled)"
fi

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
refs_rc=0
old_name_refs "$here/check-isolation.sh" "$here" "$docs" > "$work/fixed" 2> "$work/fixed.err" || refs_rc=$?
if [ "$refs_rc" -ne 0 ]; then
  bad "could not search for the old fixed network name: $(head -1 "$work/fixed.err")"
elif [ -s "$work/fixed" ]; then
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

# The two optional settings the compose file does not publish for you are documented, with the one
# fact that makes the deploy key work under cap_drop ALL: the node reads it as root without
# CAP_DAC_OVERRIDE, so the file must belong to uid 0 and be mode 0600.
# (The guide wraps lines and bolds words: read it as one line of plain text.)
flat=$(tr -d '*' < "$docs" | tr '\n' ' ' | tr -s ' ')
if grep -q 'ENDORA_GIT_PUSH_KEY_DIR' "$docs" && printf '%s' "$flat" | grep -Eq 'owned by uid 0 with mode 0600'; then
  ok "the guide names ENDORA_GIT_PUSH_KEY_DIR and says the key is uid 0, mode 0600"
else
  bad "the guide does not name ENDORA_GIT_PUSH_KEY_DIR with 'owned by uid 0' and mode 0600 (cap_drop ALL)"
fi
if grep -q 'compose.override.yaml' "$docs" && grep -Eq '"[^"]*:8788:8788"' "$docs"; then
  ok "the guide shows how to publish the team listener (8788) with an override file"
else
  bad "the guide does not show an override file publishing 8788 (compose.override.yaml, \"<address>:8788:8788\")"
fi

echo
if [ "$fail" -ne 0 ]; then
  echo "isolation check failed"
  exit 1
fi
echo "isolation check passed (KNOWN LIMIT lines are unsupported shared names, not passes of them)"
