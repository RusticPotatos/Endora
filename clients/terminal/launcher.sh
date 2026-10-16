#!/bin/sh
# Appliance recovery. The login profile must exec this launcher.
if test "$(tty 2>/dev/null)" != /dev/tty1; then exit 0; fi
trap 'exit 1' INT TERM HUP
while :; do
    "$HOME/.local/bin/endora-terminal"
    stty sane 2>/dev/null || true
    printf '\033[?1049l\033[?25h\033[2J\033[H'
    echo 'Endora recovery | R restart | Use SSH for diagnostics or boot disable.'
    while :; do
        if ! IFS= read -r choice; then exit 1; fi
        case "$choice" in r|R) break;; *) echo 'R restart';; esac
    done
done
