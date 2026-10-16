#!/bin/sh
set -eu
umask 077
test "$(uname -s)" = Linux || { echo 'Boot setup is Linux-only' >&2; exit 1; }
test "$(id -u)" -ne 0 || { echo 'Run as the device user, not root' >&2; exit 1; }
state="$HOME/.local/state/endora-terminal"
backup="$state/boot-backup"
dropin=/etc/systemd/system/getty@tty1.service.d/90-endora.conf
profile=/etc/profile.d/endora-terminal.sh
for path in "$HOME/.local" "$HOME/.local/state" "$state" "$backup"; do
    test ! -L "$path" || { echo 'Symlinked boot-state path refused' >&2; exit 1; }
done
mkdir -p "$state"
chmod 700 "$state"

restore() {
    test -f "$backup/ready" || { echo 'No completed boot backup' >&2; return 1; }
    for entry in dropin profile; do
        if test "$entry" = dropin; then target="$dropin"; else target="$profile"; fi
        if test -f "$backup/$entry"; then
            sudo install -m "$(cat "$backup/$entry-mode")" -o "$(cat "$backup/$entry-uid")" -g "$(cat "$backup/$entry-gid")" "$backup/$entry" "$target"
        else sudo rm -f "$target"; fi
    done
    sudo systemctl set-default "$(cat "$backup/target")"
    manager=$(cat "$backup/manager")
    if test -n "$manager"; then
        case "$(cat "$backup/manager-enabled")" in
            enabled|enabled-runtime) sudo systemctl enable "$manager";;
            disabled) sudo systemctl disable "$manager";;
            static|indirect|alias) :;;
            *) echo 'Unrecognized original display-manager state; restore manually' >&2; return 1;;
        esac
    fi
    sudo systemctl daemon-reload
    mv "$backup" "$state/boot-backup-restored-$(date +%s)"
    echo 'Original boot configuration restored. Reboot when ready.'
}

case "${1:-}" in
disable) restore; exit 0;;
enable) ;;
*) echo 'Use enable or disable' >&2; exit 1;;
esac
systemctl is-active --quiet ssh || { echo 'SSH must be active first' >&2; exit 1; }
systemctl is-active --quiet NetworkManager || { echo 'NetworkManager must be active first' >&2; exit 1; }
if test -f "$backup/ready"; then
    # Upgrade only our existing login hook; original-state backup remains intact.
    test ! -L "$profile" && test -f "$profile" || exit 1
    grep -qF 'endora-terminal/current/launcher.sh' "$profile" || { echo 'Managed profile changed; inspect privately before upgrading' >&2; exit 1; }
    test ! -L "$state/profile-new" || exit 1
    printf 'if [ "$(tty 2>/dev/null)" = /dev/tty1 ] && [ "$(id -u)" = %s ]; then\n  exec sh "$HOME/.local/share/endora-terminal/current/launcher.sh"\nfi\n' "$(id -u)" > "$state/profile-new"
    sudo install -m 644 "$state/profile-new" "$profile"
    rm -f "$state/profile-new"
    echo 'Existing appliance hook upgraded; original recovery backup preserved.'
    exit 0
fi
test -e /sys/class/graphics/fb0 && test -d /dev/input/by-id || { echo 'Validate display and input first' >&2; exit 1; }
test -x "$HOME/.local/bin/endora-terminal" || exit 1
user=$(id -un)
case "$user" in ''|*[!a-zA-Z0-9_-]*) echo 'Unsupported account name' >&2; exit 1;; esac
sudo -v
mkdir "$backup"
systemctl get-default > "$backup/target"
manager=$(readlink -f /etc/systemd/system/display-manager.service 2>/dev/null || true)
manager=${manager##*/}
printf '%s\n' "$manager" > "$backup/manager"
if test -n "$manager"; then systemctl is-enabled "$manager" > "$backup/manager-enabled" || true; else printf 'static\n' > "$backup/manager-enabled"; fi
case "$(cat "$backup/manager-enabled")" in enabled|disabled|static|indirect|alias) ;; *) echo 'Unsupported display-manager state' >&2; exit 1;; esac
test ! -L "$dropin" && test ! -L "$profile" || exit 1
for entry in dropin profile; do
    if test "$entry" = dropin; then source="$dropin"; else source="$profile"; fi
    if test -f "$source"; then
        sudo cat "$source" > "$backup/$entry"
        sudo stat -c %a "$source" > "$backup/$entry-mode"
        sudo stat -c %u "$source" > "$backup/$entry-uid"
        sudo stat -c %g "$source" > "$backup/$entry-gid"
    fi
done
for file in config.txt cmdline.txt; do
    if test -f "/boot/firmware/$file"; then sudo cat "/boot/firmware/$file" > "$backup/$file"; fi
done
printf 'ready\n' > "$backup/ready"
test ! -L "$state/getty-new" && test ! -L "$state/profile-new" || exit 1
# A failed enable restores captured state. No network or SSH settings are changed.
trap 'echo "Boot setup failed; restoring" >&2; restore' HUP INT TERM
printf '[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin %s --noclear %%I $TERM\n' "$user" > "$state/getty-new"
printf 'if [ "$(tty 2>/dev/null)" = /dev/tty1 ] && [ "$(id -u)" = %s ]; then\n  exec sh "$HOME/.local/share/endora-terminal/current/launcher.sh"\nfi\n' "$(id -u)" > "$state/profile-new"
if ! (sudo mkdir -p /etc/systemd/system/getty@tty1.service.d &&
    sudo install -m 644 "$state/getty-new" "$dropin" &&
    sudo install -m 644 "$state/profile-new" "$profile" &&
    sudo systemctl set-default multi-user.target &&
    { test -z "$manager" || sudo systemctl disable "$manager"; } &&
    sudo systemctl daemon-reload); then restore; exit 1; fi
rm -f "$state/getty-new" "$state/profile-new"
echo 'Boot appliance enabled. SSH and Wi-Fi are unchanged. Reboot when ready.'
