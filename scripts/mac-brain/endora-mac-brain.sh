#!/bin/sh
# The Mac's brain helper (ADR 0130). launchd runs it every thirty seconds on the owner's Mac.
# Each run:
#
#   1. reads the power source (pmset) and how long the owner has been idle (ioreg);
#   2. tells the Endora node, with a bearer token that opens that one route and nothing else
#      (POST /v1/mac/heartbeat): { plugged, idle_s, model_up }. The node answers { busy }, the
#      owner's "Mac busy" switch from the console;
#   3. keeps llama-server loaded while the Mac is plugged in and the switch is off, and stops
#      it otherwise, which gives the model's memory back to the owner.
#
# It starts and stops only the llama-server it started itself (its pid is kept, and checked to
# be a llama-server before it is signalled). It touches no other process. When the node cannot
# be reached it keeps the last busy flag it heard (not busy, if it never heard one) and goes on:
# power is local knowledge, so unplugging still unloads the model.
#
# It reads its settings from a file the install writes (see install.sh), NAME=value a line:
#
#   NODE_URL         the node's address, e.g. https://192.168.1.10:8787
#   NODE_CACERT      optional: the node's certificate, for curl --cacert
#   NODE_INSECURE    optional: 1 to skip verifying the node's certificate (a LAN only)
#   TOKEN_FILE       a file holding the helper's token, made in the console
#   LLAMA_SERVER     the llama-server binary
#   MODEL            the model file
#   LLAMA_ARGS       the rest of the command line (context size, GPU layers, ...)
#   API_KEY_FILE     a file holding the key clients must present to llama-server
#   PORT             the port llama-server listens on (8080)
#   LOG_FILE         the log, rotated by size
#   LOG_MAX          the size at which it is rotated, in bytes (1048576)
#
# For tests: ENDORA_MAC_CONFIG names the settings file, ENDORA_MAC_STATE_DIR the folder this
# keeps its pid and last busy flag in.
#
#   sh endora-mac-brain.sh          one pass
#   sh endora-mac-brain.sh stop     unloads the model it loaded (what uninstalling does)
set -u

CONFIG=${ENDORA_MAC_CONFIG:-"$HOME/Library/Application Support/Endora/mac-brain/config"}
STATE=${ENDORA_MAC_STATE_DIR:-"$HOME/Library/Application Support/Endora/mac-brain/state"}

NODE_URL=
NODE_CACERT=
NODE_INSECURE=0
TOKEN_FILE=
LLAMA_SERVER=llama-server
MODEL=
LLAMA_ARGS=
API_KEY_FILE=
PORT=8080
LOG_FILE="$HOME/Library/Logs/endora-mac-brain.log"
LOG_MAX=1048576

# Reads the settings file. Only the names above are taken, and a value is never run: a line is
# split at its first "=", nothing more.
read_config() {
  [ -f "$CONFIG" ] || return 0
  while IFS='=' read -r key value || [ -n "$key" ]; do
    case "$key" in
      NODE_URL) NODE_URL=$value ;;
      NODE_CACERT) NODE_CACERT=$value ;;
      NODE_INSECURE) NODE_INSECURE=$value ;;
      TOKEN_FILE) TOKEN_FILE=$value ;;
      LLAMA_SERVER) LLAMA_SERVER=$value ;;
      MODEL) MODEL=$value ;;
      LLAMA_ARGS) LLAMA_ARGS=$value ;;
      API_KEY_FILE) API_KEY_FILE=$value ;;
      PORT) PORT=$value ;;
      LOG_FILE) LOG_FILE=$value ;;
      LOG_MAX) LOG_MAX=$value ;;
    esac
  done <"$CONFIG"
}

# One line in the log, with the time. The log is kept small: past LOG_MAX bytes it becomes
# LOG_FILE.1 (the one before that is dropped) and a new one begins.
log() {
  mkdir -p "$(dirname "$LOG_FILE")" 2>/dev/null || return 0
  if [ -f "$LOG_FILE" ]; then
    size=$(wc -c <"$LOG_FILE" 2>/dev/null | tr -d ' ')
    if [ "${size:-0}" -ge "$LOG_MAX" ] 2>/dev/null; then
      mv -f "$LOG_FILE" "$LOG_FILE.1"
    fi
  fi
  printf '%s %s\n' "$(date '+%Y-%m-%dT%H:%M:%S')" "$*" >>"$LOG_FILE" 2>/dev/null || true
}

# true when the Mac is drawing mains power.
is_plugged() {
  pmset -g batt 2>/dev/null | head -n 1 | grep -q "AC Power"
}

# Seconds since the owner last touched the keyboard or mouse; 0 (active) when it cannot be told.
idle_seconds() {
  ns=$(ioreg -c IOHIDSystem 2>/dev/null | awk '/HIDIdleTime/ { print $NF; exit }')
  case "$ns" in
    '' | *[!0-9]*) echo 0 ;;
    *) echo $((ns / 1000000000)) ;;
  esac
}

PIDFILE="$STATE/llama.pid"
BUSYFILE="$STATE/busy"

# The pid of the llama-server this helper started, if it is still running and still one.
server_pid() {
  [ -f "$PIDFILE" ] || return 1
  pid=$(cat "$PIDFILE" 2>/dev/null)
  case "$pid" in
    '' | *[!0-9]*) rm -f "$PIDFILE"; return 1 ;;
  esac
  if kill -0 "$pid" 2>/dev/null; then
    # A pid can be reused by something else once the server is gone: it is ours only while
    # its command line still names the server.
    if ps -p "$pid" -o command= 2>/dev/null | grep -qF "$(basename "$LLAMA_SERVER")"; then
      echo "$pid"
      return 0
    fi
  fi
  rm -f "$PIDFILE"
  return 1
}

# true when the server answers its health check: its model is loaded.
model_is_up() {
  [ "$(curl -s -m 3 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/health" 2>/dev/null)" = "200" ]
}

start_server() {
  if [ -z "$MODEL" ] || [ ! -f "$MODEL" ]; then
    log "not starting: the model file is not there"
    return 0
  fi
  # llama-server reads its key from a file, so it is not on the command line for `ps` to show.
  # LLAMA_ARGS is split on spaces on purpose: it is the owner's own list of flags.
  set -- -m "$MODEL" --host 0.0.0.0 --port "$PORT"
  if [ -n "$API_KEY_FILE" ]; then
    set -- "$@" --api-key-file "$API_KEY_FILE"
  fi
  set -f
  # shellcheck disable=SC2086
  set -- "$@" $LLAMA_ARGS
  set +f
  nohup "$LLAMA_SERVER" "$@" >>"$STATE/llama-server.log" 2>&1 &
  echo $! >"$PIDFILE"
  log "started llama-server (pid $!)"
}

stop_server() {
  pid=$1
  kill "$pid" 2>/dev/null
  waited=0
  while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 10 ]; do
    sleep 1
    waited=$((waited + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid" 2>/dev/null
    log "stopped llama-server (pid $pid) by force"
  else
    log "stopped llama-server (pid $pid)"
  fi
  rm -f "$PIDFILE"
}

# Tells the node how the Mac is, and prints the owner's busy switch ("true" or "false") if it
# answered; prints nothing if it did not.
heartbeat() {
  plugged=$1
  idle=$2
  up=$3
  [ -n "$NODE_URL" ] && [ -n "$TOKEN_FILE" ] && [ -f "$TOKEN_FILE" ] || return 1
  token=$(tr -d ' \n\r' <"$TOKEN_FILE")
  [ -n "$token" ] || return 1
  reply="$STATE/heartbeat.reply"
  set -- -s -m 10 -o "$reply" -w '%{http_code}' -X POST \
    -H "Authorization: Bearer $token" -H 'Content-Type: application/json' \
    -d "{\"plugged\":$plugged,\"idle_s\":$idle,\"model_up\":$up}"
  [ -n "$NODE_CACERT" ] && set -- "$@" --cacert "$NODE_CACERT"
  [ "$NODE_INSECURE" = "1" ] && set -- "$@" -k
  code=$(curl "$@" "$NODE_URL/v1/mac/heartbeat" 2>/dev/null)
  [ "$code" = "200" ] || return 1
  if grep -q '"busy" *: *true' "$reply" 2>/dev/null; then
    echo true
  elif grep -q '"busy" *: *false' "$reply" 2>/dev/null; then
    echo false
  else
    return 1
  fi
}

main() {
  read_config
  mkdir -p "$STATE" || exit 1

  plugged=false
  is_plugged && plugged=true
  idle=$(idle_seconds)
  pid=$(server_pid) || pid=
  up=false
  if [ -n "$pid" ] && model_is_up; then
    up=true
  fi

  if busy=$(heartbeat "$plugged" "$idle" "$up"); then
    printf '%s\n' "$busy" >"$BUSYFILE"
  else
    busy=$(cat "$BUSYFILE" 2>/dev/null)
    [ "$busy" = "true" ] || busy=false
    log "the node did not answer: keeping the last busy flag ($busy)"
  fi

  wanted=false
  if [ "$plugged" = true ] && [ "$busy" = false ]; then
    wanted=true
  fi

  if [ "$wanted" = true ] && [ -z "$pid" ]; then
    start_server
  elif [ "$wanted" = false ] && [ -n "$pid" ]; then
    log "unloading: plugged=$plugged busy=$busy"
    stop_server "$pid"
  fi
}

# Unloads the model this helper loaded, and nothing else: what uninstalling does.
stop_all() {
  read_config
  mkdir -p "$STATE" || exit 1
  pid=$(server_pid) || pid=
  [ -z "$pid" ] || stop_server "$pid"
}

case "${1:-run}" in
  run) main ;;
  stop) stop_all ;;
  *) echo "usage: endora-mac-brain.sh [run | stop]" >&2; exit 2 ;;
esac
exit 0
