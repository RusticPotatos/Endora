#!/bin/sh
# On-demand graphical session. All cookies and Access caches are temporary.
set -eu
umask 077
case "${1:-}" in portal|web|auth) mode=$1;; *) exit 1;; esac
case "${2:-}" in http://*|https://*) url=$2;; *) exit 1;; esac
bundle=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
runtime=${XDG_RUNTIME_DIR:-}
test -n "$runtime" && test -d "$runtime" && test ! -L "$runtime" || { echo 'Browser needs a volatile user runtime; use the handheld login.' >&2; exit 1; }
test "$(findmnt -n -o FSTYPE --target "$runtime")" = tmpfs || { echo 'Browser authentication requires volatile storage.' >&2; exit 1; }
session=$(mktemp -d "$runtime/endora-browser.XXXXXXXX")
cleanup() { rm -rf -- "$session"; }
trap cleanup EXIT HUP INT TERM
export ENDORA_BROWSER_SESSION="$session"
# Reuse the login console the user owns; a different getty's VT is root-owned.
console=$(tty)
case "$console" in /dev/tty[0-9]*) number=${console#/dev/tty};; *) echo 'Open the browser from the handheld console.' >&2; exit 1;; esac
case "$number" in ''|*[!0-9]*) exit 1;; esac
# No listening X socket, no desktop startup, no browser sandbox bypass.
xinit "$bundle/browser-session.sh" "$mode" "$url" -- :1 "vt$number" -keeptty -nolisten tcp
