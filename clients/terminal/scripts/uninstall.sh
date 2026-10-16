#!/bin/sh
set -eu
base="$HOME/.local/share/endora-terminal"
alias="$HOME/.local/bin/endora"
if test -L "$alias" && test "$(readlink "$alias")" = "$HOME/.local/bin/endora-terminal"; then rm "$alias"; fi
desktop="$HOME/.local/share/applications/endora-terminal.desktop"
if test -f "$desktop" && test ! -L "$desktop" && grep -q '^X-Endora-Managed=true$' "$desktop"; then rm "$desktop"; fi
if test -f "$HOME/.local/state/endora-terminal/boot-backup/ready"; then
    sh "$base/current/boot.sh" disable
fi
for icon in "$HOME/Desktop/Back to Endora.desktop" "$HOME/.local/share/applications/Back to Endora.desktop"; do
    if test -f "$icon" && test ! -L "$icon" && grep -q '^Exec=sudo -n chvt 1$' "$icon"; then rm "$icon"; fi
done
probe=/etc/NetworkManager/conf.d/80-endora-connectivity.conf
if test -f "$probe" && test ! -L "$probe" && grep -q '^# Endora managed connectivity probe$' "$probe"; then
    sudo rm "$probe"
    sudo nmcli general reload conf
fi
if test -f /etc/sudoers.d/endora-backlight && test -f /usr/local/libexec/endora-backlight; then
    if cmp -s "$base/current/backlight-helper.sh" /usr/local/libexec/endora-backlight; then
        sudo rm /etc/sudoers.d/endora-backlight /usr/local/libexec/endora-backlight
    fi
fi
test ! -L "$base" || exit 1
test -L "$HOME/.local/bin/endora-terminal" && rm "$HOME/.local/bin/endora-terminal"
rm -rf "$base"
echo 'Application removed; private configuration and session retained. Run logout first to remove the local session.'
