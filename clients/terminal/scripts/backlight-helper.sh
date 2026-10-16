#!/bin/sh
# Install as root-owned /usr/local/libexec/endora-backlight.
set -eu
test "$#" -eq 1 || exit 1
base=/sys/class/backlight/backlight
max=$(cat "$base/max_brightness")
case "$max" in ''|*[!0-9]*) exit 1;; esac
case "$1" in
on) value=$max;;
dim) value=$((max / 3)); test "$value" -gt 0 || value=$max;;
off) value=0;;
*) exit 1;;
esac
printf '%s\n' "$value" > "$base/brightness"
