#!/bin/sh
# Show the Raspberry Pi desktop on its own virtual terminal and come back to Endora afterwards.
# Return with Alt + Back, the "Back to Endora" icon, or `sudo chvt 1` over SSH. Nothing is
# enabled at boot, and the desktop is stopped again when you return.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
active() { cat /sys/class/tty/tty0/active; }
test "$(id -u)" != 0 || exit 1
test "$(active)" = tty1 || { echo 'Start the desktop from the Endora console.' >&2; exit 1; }

# A visible way back, created only if missing.
mkdir -p "$HOME/Desktop" "$HOME/.local/share/applications"
for dir in "$HOME/Desktop" "$HOME/.local/share/applications"; do
    icon="$dir/Back to Endora.desktop"
    if test ! -e "$icon"; then
        printf '[Desktop Entry]\nType=Application\nName=Back to Endora\nComment=Return to the Endora console\nExec=sudo -n chvt 1\nIcon=go-home\nTerminal=false\n' > "$icon"
        chmod 755 "$icon"
    fi
done

watcher=
cleanup() {
    trap - EXIT HUP INT TERM
    test -z "$watcher" || kill "$watcher" 2>/dev/null || true
    sudo -n systemctl stop lightdm 2>/dev/null || true
    sudo -n chvt 1 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM

sudo -n systemctl start lightdm
tries=0
while test "$(active)" = tty1; do
    tries=$((tries + 1))
    test "$tries" -le 40 || { echo 'The desktop did not start.' >&2; exit 1; }
    test "$tries" -ne 6 || sudo -n chvt 7 || true
    sleep 1
done

sudo -n python3 "$here/desktop-watch.py" &
watcher=$!
while test "$(active)" != tty1; do sleep 1; done
