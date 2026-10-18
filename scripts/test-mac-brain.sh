#!/usr/bin/env sh
# Tests for scripts/mac-brain/endora-mac-brain.sh, the Mac's brain helper (ADR 0130), and for
# what scripts/mac-brain/install.sh renders. pmset, ioreg, curl and llama-server are stubs on the
# PATH, so nothing here needs a Mac, a model, a network, a launchd or root, and no real
# process but the stubs' own is ever started or stopped.
#
#   sh scripts/test-mac-brain.sh
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
HELPER="$HERE/mac-brain/endora-mac-brain.sh"
INSTALL="$HERE/mac-brain/install.sh"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/mac-brain-test.XXXXXX")

cleanup() {
  for pidfile in "$WORK"/case-*/state/llama.pid "$WORK"/case-*/state2/llama.pid; do
    [ -f "$pidfile" ] && kill "$(cat "$pidfile")" 2>/dev/null
  done
  for decoy in "$WORK"/case-*/decoy.pid; do
    [ -f "$decoy" ] && kill "$(cat "$decoy")" 2>/dev/null
  done
  rm -rf "$WORK"
}
trap cleanup EXIT

FAILED=0
expect() {
  # $1 what, $2 the status of the check (0 is a pass)
  if [ "$2" -eq 0 ]; then
    echo "  ok    $1"
  else
    echo "  FAIL  $1"
    FAILED=1
  fi
}

CASE=0

# A fresh scratch Mac: stubs on a PATH, a settings file, a power source, an idle time, a node
# that answers { "busy": false } and a server whose health is 200.
fresh() {
  CASE=$((CASE + 1))
  T="$WORK/case-$CASE"
  S="$T/stub"
  mkdir -p "$T/bin" "$T/state" "$T/home" "$S"
  printf 'abc123\n' >"$T/token"
  printf 'the-servers-key\n' >"$T/key"
  : >"$T/model.gguf"
  LOG_MAX=1048576
  write_config
  ac
  idle_ns 7000000000
  printf '{"busy":false}' >"$S/reply"
  printf '200' >"$S/health"

  cat >"$T/bin/pmset" <<EOF
#!/bin/sh
cat "$S/pmset"
EOF
  cat >"$T/bin/ioreg" <<EOF
#!/bin/sh
[ -f "$S/idle_ns" ] && printf '    | |   "HIDIdleTime" = %s\n' "\$(cat "$S/idle_ns")"
exit 0
EOF
  cat >"$T/bin/curl" <<EOF
#!/bin/sh
out=; data=; url=; auth=
while [ \$# -gt 0 ]; do
  case "\$1" in
    -o) out=\$2; shift 2 ;;
    -d) data=\$2; shift 2 ;;
    -H) case "\$2" in Authorization:*) auth=\$2 ;; esac; shift 2 ;;
    -w | -m | -X | --cacert) shift 2 ;;
    http*) url=\$1; shift ;;
    *) shift ;;
  esac
done
case "\$url" in
  */health) printf '%s' "\$(cat "$S/health")" ;;
  */v1/mac/heartbeat)
    [ -f "$S/node-down" ] && exit 7
    printf 'heartbeat | %s | %s | %s\n' "\$url" "\$auth" "\$data" >>"$S/curl.log"
    cat "$S/reply" >"\$out"
    printf '200'
    ;;
esac
exit 0
EOF
  # The server stub records how it was started and stays up until it is stopped.
  cat >"$T/bin/llama-server" <<EOF
#!/bin/sh
echo "\$*" >>"$S/server.args"
trap 'exit 0' TERM
while :; do sleep 1; done
EOF
  chmod +x "$T/bin/pmset" "$T/bin/ioreg" "$T/bin/curl" "$T/bin/llama-server"
}

write_config() {
  cat >"$T/config" <<EOF
NODE_URL=https://192.168.1.10:8787
TOKEN_FILE=$T/token
LLAMA_SERVER=$T/bin/llama-server
MODEL=$T/model.gguf
LLAMA_ARGS=-c 8192 -ngl 99 --jinja
API_KEY_FILE=$T/key
PORT=8080
LOG_FILE=$T/home/Library/Logs/endora-mac-brain.log
LOG_MAX=$LOG_MAX
EOF
}

ac() { printf "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%%; charged; 0:00 remaining present: true\n" >"$S/pmset"; }
battery() { printf "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t80%%; discharging; 4:10 remaining present: true\n" >"$S/pmset"; }
idle_ns() { printf '%s' "$1" >"$S/idle_ns"; }
node_down() { : >"$S/node-down"; }
node_up() { rm -f "$S/node-down"; }
node_says() { printf '%s' "$1" >"$S/reply"; }

run() {
  HOME="$T/home" PATH="$T/bin:$PATH" ENDORA_MAC_CONFIG="$T/config" ENDORA_MAC_STATE_DIR="$T/state" \
    sh "$HELPER" >"$T/out" 2>&1
  STATUS=$?
  # A server just started writes its arguments a moment later: wait for them.
  if [ -f "$T/state/llama.pid" ] && [ ! -f "$S/server.args" ]; then
    waited=0
    while [ ! -f "$S/server.args" ] && [ "$waited" -lt 30 ]; do
      sleep 0.1
      waited=$((waited + 1))
    done
  fi
}

server_alive() {
  [ -f "$T/state/llama.pid" ] && kill -0 "$(cat "$T/state/llama.pid")" 2>/dev/null
}
starts() { [ -f "$S/server.args" ] && wc -l <"$S/server.args" | tr -d ' ' || echo 0; }
last_beat() { tail -n 1 "$S/curl.log" 2>/dev/null; }
has() { grep -qF -- "$2" "$1"; }
logged() { has "$T/home/Library/Logs/endora-mac-brain.log" "$1"; }

echo "plugged in and not busy: the model is loaded"

fresh
run
expect "starts llama-server" "$([ $STATUS -eq 0 ] && server_alive; echo $?)"
expect "  from the model file, listening on the LAN on port 8080" "$(has "$S/server.args" "-m $T/model.gguf --host 0.0.0.0 --port 8080"; echo $?)"
expect "  with its key in a file, not on the command line" "$(has "$S/server.args" "--api-key-file $T/key" && ! has "$S/server.args" 'the-servers-key'; echo $?)"
expect "  and the owner's own flags after" "$(has "$S/server.args" '-c 8192 -ngl 99 --jinja'; echo $?)"
expect "  the first heartbeat says it is plugged in and the model is not up yet" "$(last_beat | grep -qF '"plugged":true' && last_beat | grep -qF '"model_up":false'; echo $?)"
run
expect "a second pass does not start another" "$([ "$(starts)" = 1 ] && server_alive; echo $?)"
expect "  and says the model is up once it answers its health check" "$(last_beat | grep -qF '"model_up":true'; echo $?)"
printf '503' >"$S/health"
run
expect "  but not while it is still loading (health 503)" "$(last_beat | grep -qF '"model_up":false'; echo $?)"

echo "the heartbeat"

fresh
idle_ns 125000000000
run
expect "reports the idle seconds from ioreg (125 s)" "$(last_beat | grep -qF '"idle_s":125'; echo $?)"
expect "  to the node's heartbeat route" "$(last_beat | grep -qF 'https://192.168.1.10:8787/v1/mac/heartbeat'; echo $?)"
expect "  with the helper's token as a bearer, newline stripped" "$(last_beat | grep -qF 'Authorization: Bearer abc123 |'; echo $?)"
fresh
rm -f "$S/idle_ns"
run
expect "an ioreg that says nothing counts as the owner active (0 s)" "$(last_beat | grep -qF '"idle_s":0'; echo $?)"
fresh
battery
run
expect "reports a battery as not plugged" "$(last_beat | grep -qF '"plugged":false'; echo $?)"

echo "on battery: the model is unloaded"

fresh
run
LOADED=$(cat "$T/state/llama.pid")
battery
run
expect "stops the llama-server it started" "$(! kill -0 "$LOADED" 2>/dev/null && [ ! -f "$T/state/llama.pid" ]; echo $?)"
expect "  and says so in the log" "$(logged 'unloading: plugged=false'; echo $?)"
fresh
battery
run
expect "on battery from the start, nothing is started" "$([ "$(starts)" = 0 ]; echo $?)"

fresh
run
LOADED=$(cat "$T/state/llama.pid")
HOME="$T/home" PATH="$T/bin:$PATH" ENDORA_MAC_CONFIG="$T/config" ENDORA_MAC_STATE_DIR="$T/state" sh "$HELPER" stop >/dev/null 2>&1
expect "stop (what uninstalling does) unloads the model it loaded" "$(! kill -0 "$LOADED" 2>/dev/null && [ ! -f "$T/state/llama.pid" ]; echo $?)"

echo "the console's Mac busy switch"

fresh
run
expect "(loaded first)" "$(server_alive; echo $?)"
node_says '{"busy":true}'
run
expect "a node that says busy unloads the model" "$(! server_alive; echo $?)"
expect "  and it stays unloaded while the switch is on, plugged in or not" "$(run; [ "$(starts)" = 1 ] && ! server_alive; echo $?)"
node_says '{"busy": false}'
run
expect "  until the switch is turned off" "$([ "$(starts)" = 2 ] && server_alive; echo $?)"

echo "a node that cannot be reached"

fresh
run
node_down
run
expect "the model stays loaded on the last busy flag (not busy)" "$(server_alive && [ "$STATUS" -eq 0 ]; echo $?)"
expect "  and the helper does not crash, and says why" "$(logged 'the node did not answer'; echo $?)"
fresh
run
node_says '{"busy":true}'
run
node_down
run
expect "a busy flag it heard is remembered while the node is away: the model stays unloaded" "$(! server_alive; echo $?)"
fresh
printf 'true\n' >"$T/state/busy"
node_down
run
expect "a busy flag heard earlier is kept while the node is away: nothing is started" "$([ "$(starts)" = 0 ] && ! server_alive; echo $?)"
fresh
node_down
run
expect "with no flag ever heard, it is not busy: the model loads" "$(server_alive; echo $?)"
battery
run
expect "unplugging still unloads it with the node away" "$(! server_alive; echo $?)"

echo "it touches no other process"

fresh
(sleep 300 >/dev/null 2>&1 & echo $! >"$T/decoy.pid")
cp "$T/decoy.pid" "$T/state/llama.pid"
battery
run
expect "a pid that is not a llama-server is never signalled" "$(kill -0 "$(cat "$T/decoy.pid")" 2>/dev/null; echo $?)"
expect "  and the stale pid file is dropped" "$([ ! -f "$T/state/llama.pid" ]; echo $?)"
fresh
(sleep 300 >/dev/null 2>&1 & echo $! >"$T/decoy.pid")
cp "$T/decoy.pid" "$T/state/llama.pid"
run
expect "plugged in over a stale pid: its own server starts and the stranger lives" "$(kill -0 "$(cat "$T/decoy.pid")" 2>/dev/null && server_alive && [ "$(cat "$T/state/llama.pid")" != "$(cat "$T/decoy.pid")" ]; echo $?)"
fresh
printf 'not-a-number\n' >"$T/state/llama.pid"
battery
run
expect "a damaged pid file is dropped, not run" "$([ $STATUS -eq 0 ] && [ ! -f "$T/state/llama.pid" ]; echo $?)"

echo "the model file and the token"

fresh
rm -f "$T/model.gguf"
run
expect "no model file: nothing is started, and it says so" "$([ "$(starts)" = 0 ] && logged 'the model file is not there'; echo $?)"
fresh
rm -f "$T/token"
run
expect "no token file: no heartbeat is sent, and the model is still looked after" "$([ ! -f "$S/curl.log" ] && server_alive; echo $?)"

echo "the log"

fresh
LOG_MAX=200
write_config
node_down
for _ in 1 2 3 4 5 6 7 8; do run; done
LOG="$T/home/Library/Logs/endora-mac-brain.log"
expect "is rotated by size: one older file is kept" "$([ -f "$LOG.1" ] && [ ! -f "$LOG.2" ]; echo $?)"
expect "  and the current one stays small" "$([ "$(wc -c <"$LOG" | tr -d ' ')" -lt 400 ]; echo $?)"

echo "the install renders the helper's files"

fresh
OUTDIR="$T/rendered"
ENDORA_MAC_NODE_URL='https://192.168.1.10:8787' ENDORA_MAC_TOKEN_FILE="$T/token" ENDORA_MAC_MODEL="$T/model.gguf" \
  ENDORA_MAC_KEY_FILE="$T/key" ENDORA_MAC_LLAMA_SERVER="$T/bin/llama-server" ENDORA_MAC_LLAMA_ARGS='-c 4096 -ngl 99' \
  sh "$INSTALL" render "$OUTDIR" >"$T/install.out" 2>&1
RENDER=$?
PLIST="$OUTDIR/net.endora.mac-brain.plist"
CONF="$OUTDIR/config"
expect "renders a helper, a settings file and a LaunchAgent" "$([ $RENDER -eq 0 ] && [ -f "$OUTDIR/endora-mac-brain.sh" ] && [ -f "$CONF" ] && [ -f "$PLIST" ]; echo $?)"
expect "  the settings carry the owner's values" "$(has "$CONF" 'NODE_URL=https://192.168.1.10:8787' && has "$CONF" "MODEL=$T/model.gguf" && has "$CONF" 'LLAMA_ARGS=-c 4096 -ngl 99' && has "$CONF" 'PORT=8080'; echo $?)"
expect "  the agent runs the helper every 30 seconds" "$(has "$PLIST" '<key>StartInterval</key>' && has "$PLIST" '<integer>30</integer>' && has "$PLIST" "$OUTDIR/endora-mac-brain.sh"; echo $?)"
expect "  and lets the model it started outlive the pass" "$(has "$PLIST" '<key>AbandonProcessGroup</key>'; echo $?)"
expect "  with nothing left unfilled" "$(! grep -q '@[A-Z_]*@' "$PLIST" "$CONF"; echo $?)"
expect "  and the settings file readable by its owner only" "$([ "$(ls -l "$CONF" | cut -c1-10)" = "-rw-------" ]; echo $?)"
if command -v plutil >/dev/null 2>&1; then
  expect "  and the plist is valid" "$(plutil -lint "$PLIST" >/dev/null 2>&1; echo $?)"
fi
# What the install wrote is what the helper reads.
HOME="$T/home" PATH="$T/bin:$PATH" ENDORA_MAC_CONFIG="$CONF" ENDORA_MAC_STATE_DIR="$T/state2" sh "$OUTDIR/endora-mac-brain.sh" >/dev/null 2>&1
expect "  the rendered helper and settings work together" "$([ -f "$T/state2/llama.pid" ] && kill -0 "$(cat "$T/state2/llama.pid")" 2>/dev/null; echo $?)"
kill "$(cat "$T/state2/llama.pid" 2>/dev/null)" 2>/dev/null

fresh
ENDORA_MAC_NODE_URL='https://192.168.1.10:8787' ENDORA_MAC_TOKEN_FILE="$T/token" ENDORA_MAC_MODEL="$T/model.gguf" \
  ENDORA_MAC_KEY_FILE="$T/key" ENDORA_MAC_LLAMA_SERVER="$T/bin/llama-server" \
  ENDORA_MAC_LLAMA_ARGS='-c 4096 & <b>x</b>' sh "$INSTALL" render "$T/odd&dir" >/dev/null 2>&1
expect "an & in a path cannot break the plist's XML" "$(has "$T/odd&dir/net.endora.mac-brain.plist" 'odd&amp;dir/endora-mac-brain.sh'; echo $?)"
expect "  and a value goes into the settings as it is" "$(has "$T/odd&dir/config" 'LLAMA_ARGS=-c 4096 & <b>x</b>'; echo $?)"

fresh
: >"$T/home/m.gguf"
HOME="$T/home" ENDORA_MAC_NODE_URL='https://192.168.1.10:8787' ENDORA_MAC_TOKEN_FILE='~/token' ENDORA_MAC_MODEL='~/m.gguf' \
  ENDORA_MAC_KEY_FILE='~/key' ENDORA_MAC_LLAMA_SERVER="$T/bin/llama-server" sh "$INSTALL" render "$T/tilde" >/dev/null 2>&1
expect "a leading ~/ in a path is the owner's home" "$(has "$T/tilde/config" "MODEL=$T/home/m.gguf" && has "$T/tilde/config" "TOKEN_FILE=$T/home/token"; echo $?)"

fresh
env -i PATH="$PATH" HOME="$T/home" sh "$INSTALL" render "$T/none" >"$T/none.out" 2>&1
RC=$?
expect "a missing setting is named, and nothing is written" "$([ $RC -ne 0 ] && grep -q 'ENDORA_MAC_NODE_URL' "$T/none.out" && [ ! -e "$T/none/config" ]; echo $?)"
fresh
ENDORA_MAC_NODE_URL='https://192.168.1.10:8787' ENDORA_MAC_TOKEN_FILE="$T/token" ENDORA_MAC_MODEL="$T/nowhere.gguf" \
  ENDORA_MAC_KEY_FILE="$T/key" ENDORA_MAC_LLAMA_SERVER="$T/bin/llama-server" sh "$INSTALL" render "$T/nomodel" >"$T/nomodel.out" 2>&1
RC=$?
expect "a model file that is not there is refused by name" "$([ $RC -ne 0 ] && grep -q 'ENDORA_MAC_MODEL' "$T/nomodel.out"; echo $?)"

if [ "$FAILED" -ne 0 ]; then
  echo "mac-brain: FAILED"
  exit 1
fi
echo "mac-brain: all passed"
