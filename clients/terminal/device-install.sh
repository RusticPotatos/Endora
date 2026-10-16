#!/bin/sh
# Explicit optional device support; never alters network profiles or drivers.
set -eu
test "$(uname -s)" = Linux || exit 1
bundle=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
user=$(id -un)
case "$user" in ''|*[!a-zA-Z0-9_-]*) exit 1;; esac
sudo -v
sudo mkdir -p /usr/local/libexec
sudo install -o root -g root -m 755 "$bundle/backlight-helper.sh" /usr/local/libexec/endora-backlight
temp=$(mktemp)
trap 'rm -f "$temp"' EXIT HUP INT TERM
printf '%s ALL=(root) NOPASSWD: /usr/local/libexec/endora-backlight on, /usr/local/libexec/endora-backlight dim, /usr/local/libexec/endora-backlight off\n' "$user" > "$temp"
sudo visudo -cf "$temp" >/dev/null
sudo install -o root -g root -m 440 "$temp" /etc/sudoers.d/endora-backlight
connectivity=/etc/NetworkManager/conf.d/80-endora-connectivity.conf
if test -e "$connectivity"; then
    test ! -L "$connectivity" && grep -q '^# Endora managed connectivity probe$' "$connectivity" || { echo 'Existing probe file is unmanaged; preserved.' >&2; exit 1; }
fi
cat > "$temp" <<'CONF'
# Endora managed connectivity probe
[connectivity]
enabled=true
uri=http://network-test.debian.org/nm
response=NetworkManager is online
interval=60
CONF
sudo install -o root -g root -m 644 "$temp" "$connectivity"
sudo nmcli general reload conf
echo 'Backlight and Debian portal-detection probe installed; saved networks are preserved.'
