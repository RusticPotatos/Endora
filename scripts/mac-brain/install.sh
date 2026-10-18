#!/bin/sh
# Installs the Mac's brain helper (ADR 0130) as a LaunchAgent on the owner's Mac. No sudo: it
# writes under the owner's home and asks launchd for the owner's own agent.
#
#   sh scripts/mac-brain/install.sh install     renders, then loads the agent
#   sh scripts/mac-brain/install.sh uninstall   unloads the agent, stops the model it loaded
#   sh scripts/mac-brain/install.sh render DIR  only writes the files into DIR (what the tests use)
#
# The owner's values come from the environment, which `make mac-brain-install` fills from
# local.mk (git-ignored): nothing about the owner's network or files is in the repository.
#
#   ENDORA_MAC_NODE_URL       the node's address, e.g. https://192.168.1.10:8787     (required)
#   ENDORA_MAC_TOKEN_FILE     where the helper's token is kept; the console makes it   (required)
#   ENDORA_MAC_MODEL          the model file, a .gguf                                  (required)
#   ENDORA_MAC_KEY_FILE       the key llama-server asks of its clients; made if absent (required)
#   ENDORA_MAC_LLAMA_SERVER   llama-server (default: the one on the PATH)
#   ENDORA_MAC_LLAMA_ARGS     its other flags (default: -ngl 99 -c 32768)
#   ENDORA_MAC_PORT           its port (default: 8080)
#   ENDORA_MAC_NODE_CACERT    the node's certificate, if it is self-signed
#   ENDORA_MAC_NODE_INSECURE  1 to skip checking the node's certificate (a trusted LAN only)
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
LABEL=net.endora.mac-brain
DIR="$HOME/Library/Application Support/Endora/mac-brain"
AGENTS="$HOME/Library/LaunchAgents"

say() { printf '%s\n' "$*"; }
die() {
  printf 'mac-brain: %s\n' "$*" >&2
  exit 1
}

# A leading ~/ in a path is the owner's home: make does not expand it, and the helper's
# settings must hold whole paths.
untilde() {
  case "$1" in
    "~/"*) printf '%s' "$HOME/${1#\~/}" ;;
    *) printf '%s' "$1" ;;
  esac
}
ENDORA_MAC_TOKEN_FILE=$(untilde "${ENDORA_MAC_TOKEN_FILE:-}")
ENDORA_MAC_MODEL=$(untilde "${ENDORA_MAC_MODEL:-}")
ENDORA_MAC_KEY_FILE=$(untilde "${ENDORA_MAC_KEY_FILE:-}")
ENDORA_MAC_NODE_CACERT=$(untilde "${ENDORA_MAC_NODE_CACERT:-}")
ENDORA_MAC_LLAMA_SERVER=$(untilde "${ENDORA_MAC_LLAMA_SERVER:-}")

# Names the settings that are missing, all at once, so one run is enough to fix them.
check_settings() {
  missing=
  for name in ENDORA_MAC_NODE_URL ENDORA_MAC_TOKEN_FILE ENDORA_MAC_MODEL ENDORA_MAC_KEY_FILE; do
    eval "value=\${$name:-}"
    [ -n "$value" ] || missing="$missing $name"
  done
  [ -z "$missing" ] || die "set these in local.mk first:$missing"
  [ -f "$ENDORA_MAC_MODEL" ] || die "ENDORA_MAC_MODEL: there is no file at $ENDORA_MAC_MODEL"
  for name in ENDORA_MAC_NODE_URL ENDORA_MAC_TOKEN_FILE ENDORA_MAC_MODEL ENDORA_MAC_KEY_FILE \
    ENDORA_MAC_LLAMA_SERVER ENDORA_MAC_LLAMA_ARGS ENDORA_MAC_NODE_CACERT; do
    eval "value=\${$name:-}"
    case "$value" in
      *"
"*) die "$name has a line break in it" ;;
    esac
  done
  case "$ENDORA_MAC_NODE_URL" in
    http://* | https://*) ;;
    *) die "ENDORA_MAC_NODE_URL starts with http:// or https://" ;;
  esac
  case "${ENDORA_MAC_PORT:-8080}" in
    '' | *[!0-9]*) die "ENDORA_MAC_PORT is a port number" ;;
  esac
}

# Fills the plist template. Each value goes in whole, XML-escaped, wherever its @NAME@ stands.
fill() {
  V_HELPER=$1 V_CONFIG=$2 V_LOG=$3 awk '
    function xml(s) { gsub(/&/, "\\&amp;", s); gsub(/</, "\\&lt;", s); gsub(/>/, "\\&gt;", s); return s }
    function put(line, tag, val,   out, i) {
      out = ""
      while ((i = index(line, tag)) > 0) { out = out substr(line, 1, i - 1) val; line = substr(line, i + length(tag)) }
      return out line
    }
    {
      line = put($0, "@HELPER@", xml(ENVIRON["V_HELPER"]))
      line = put(line, "@CONFIG@", xml(ENVIRON["V_CONFIG"]))
      line = put(line, "@LOG@", xml(ENVIRON["V_LOG"]))
      print line
    }' "$HERE/$LABEL.plist.template"
}

# Writes the helper, its settings and the agent into $1.
render() {
  out=$1
  check_settings
  server=${ENDORA_MAC_LLAMA_SERVER:-$(command -v llama-server 2>/dev/null)}
  [ -n "$server" ] || die "llama-server is not installed: brew install llama.cpp"
  mkdir -p "$out" || die "cannot write to $out"
  cp "$HERE/endora-mac-brain.sh" "$out/endora-mac-brain.sh" || die "cannot copy the helper"
  chmod 755 "$out/endora-mac-brain.sh"
  # The settings name files that hold secrets, never the secrets: still owner-only.
  (
    umask 077
    {
      printf 'NODE_URL=%s\n' "$ENDORA_MAC_NODE_URL"
      printf 'NODE_CACERT=%s\n' "${ENDORA_MAC_NODE_CACERT:-}"
      printf 'NODE_INSECURE=%s\n' "${ENDORA_MAC_NODE_INSECURE:-0}"
      printf 'TOKEN_FILE=%s\n' "$ENDORA_MAC_TOKEN_FILE"
      printf 'LLAMA_SERVER=%s\n' "$server"
      printf 'MODEL=%s\n' "$ENDORA_MAC_MODEL"
      printf 'LLAMA_ARGS=%s\n' "${ENDORA_MAC_LLAMA_ARGS--ngl 99 -c 32768}"
      printf 'API_KEY_FILE=%s\n' "$ENDORA_MAC_KEY_FILE"
      printf 'PORT=%s\n' "${ENDORA_MAC_PORT:-8080}"
    } >"$out/config"
  ) || die "cannot write the settings"
  chmod 600 "$out/config"
  fill "$out/endora-mac-brain.sh" "$out/config" "$HOME/Library/Logs/endora-mac-brain.launchd.log" \
    >"$out/$LABEL.plist" || die "cannot write the agent"
  say "rendered the helper, its settings and the agent into $out"
}

domain() { printf 'gui/%s' "$(id -u)"; }

case "${1:-}" in
  render)
    [ -n "${2:-}" ] || die "usage: install.sh render DIR"
    render "$2"
    ;;
  install)
    [ "$(uname)" = "Darwin" ] || die "this installs a LaunchAgent: run it on the Mac"
    render "$DIR"
    # The key llama-server will ask clients for: made once, owner-only, and never printed.
    if [ ! -f "$ENDORA_MAC_KEY_FILE" ]; then
      (umask 077 && openssl rand -hex 24 >"$ENDORA_MAC_KEY_FILE") || die "cannot make $ENDORA_MAC_KEY_FILE"
      say "made the model's key in $ENDORA_MAC_KEY_FILE: read it there and enter it in the console"
    fi
    mkdir -p "$AGENTS" "$HOME/Library/Logs"
    cp "$DIR/$LABEL.plist" "$AGENTS/$LABEL.plist" || die "cannot write $AGENTS/$LABEL.plist"
    launchctl bootout "$(domain)/$LABEL" >/dev/null 2>&1
    launchctl bootstrap "$(domain)" "$AGENTS/$LABEL.plist" || die "launchd would not load the agent"
    say "loaded $LABEL: it looks every 30 seconds. Its log is ~/Library/Logs/endora-mac-brain.log"
    [ -f "$ENDORA_MAC_TOKEN_FILE" ] || say "no token yet: make one in the console (HQ, The Mac) and save it in $ENDORA_MAC_TOKEN_FILE"
    ;;
  uninstall)
    [ "$(uname)" = "Darwin" ] || die "this removes a LaunchAgent: run it on the Mac"
    launchctl bootout "$(domain)/$LABEL" >/dev/null 2>&1
    rm -f "$AGENTS/$LABEL.plist"
    # The model the helper loaded goes with it; the settings, key and logs stay.
    if [ -f "$DIR/config" ]; then
      ENDORA_MAC_CONFIG="$DIR/config" sh "$DIR/endora-mac-brain.sh" stop
    fi
    say "removed $LABEL. The settings in $DIR and the logs are kept."
    ;;
  *)
    die "usage: install.sh install | uninstall | render DIR"
    ;;
esac
