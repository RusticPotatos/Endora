#!/bin/sh
# A locked portal browser cannot see the user's sessions, SSH keys or home files.
set -eu
umask 077
command -v bwrap >/dev/null || { echo 'Portal isolation needs the Debian bubblewrap package.' >&2; exit 1; }
session=${ENDORA_BROWSER_SESSION:?}
test -d "$session"
authority=${XAUTHORITY:-$HOME/.Xauthority}
test -f "$authority" || { echo 'Graphical session authentication unavailable.' >&2; exit 1; }
mkdir -p "$session/portal"
cp "$authority" "$session/portal/Xauthority"
# Mount only browser runtime files, networking configuration and this session's
# display cookie. No host home, /run, /root, removable media or input devices.
set -- bwrap --die-with-parent --new-session --unshare-all --share-net \
    --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/sbin /sbin \
    --symlink usr/lib /lib --proc /proc --dev /dev --tmpfs /tmp --tmpfs /home \
    --bind "$session/portal" /home/portal \
    --ro-bind /tmp/.X11-unix /tmp/.X11-unix \
    --ro-bind /etc/resolv.conf /etc/resolv.conf \
    --ro-bind /etc/hosts /etc/hosts \
    --ro-bind /etc/nsswitch.conf /etc/nsswitch.conf \
    --ro-bind /etc/ssl /etc/ssl --ro-bind /etc/fonts /etc/fonts \
    --clearenv --setenv HOME /home/portal --setenv PATH /usr/bin:/bin \
    --setenv DISPLAY "${DISPLAY:?}" --setenv XAUTHORITY /home/portal/Xauthority
for path in /etc/chromium /etc/chromium.d /etc/chromium-browser; do
    if test -d "$path"; then set -- "$@" --ro-bind "$path" "$path"; fi
done
exec "$@" chromium-browser --user-data-dir=/home/portal/profile \
    --no-first-run --disable-sync --disable-dev-shm-usage --start-maximized "${ENDORA_PORTAL_URL:?}"
