#!/bin/sh
# A text-mode browser inside the terminal, for Wi-Fi login pages ("accept" portals) and quick
# reading. It runs in a sandbox that cannot see the device's files, sessions or keys, and keeps
# nothing: cookies live in memory-backed storage that disappears when it closes.
#   Arrows move between links, Enter follows one, Tab/Down reach form buttons,
#   Q quits (press Y to confirm) and Backspace shows the history.
set -eu
umask 077
case "${1:-}" in portal|web) ;; *) exit 1;; esac
case "${2:-}" in http://*|https://*) url=$2;; *) exit 1;; esac
command -v bwrap >/dev/null || { echo 'Needs the Debian bubblewrap package.' >&2; exit 1; }
command -v lynx >/dev/null || { echo 'Needs the Debian lynx package.' >&2; exit 1; }
set -- bwrap --die-with-parent --unshare-all --share-net \
    --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/sbin /sbin \
    --symlink usr/lib /lib --proc /proc --dev /dev --tmpfs /tmp --tmpfs /home \
    --ro-bind /etc/resolv.conf /etc/resolv.conf --ro-bind /etc/hosts /etc/hosts \
    --ro-bind /etc/nsswitch.conf /etc/nsswitch.conf --ro-bind /etc/ssl /etc/ssl \
    --clearenv --setenv HOME /home/lynx --setenv PATH /usr/bin:/bin \
    --setenv TERM "${TERM:-linux}" --setenv LANG "${LANG:-C.UTF-8}"
if test -d /etc/lynx; then set -- "$@" --ro-bind /etc/lynx /etc/lynx; fi
exec "$@" lynx -accept_all_cookies -useragent="Mozilla/5.0 (X11; Linux aarch64) Lynx" -nolist "$url"
