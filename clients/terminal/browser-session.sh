#!/bin/sh
set -eu
umask 077
session=${ENDORA_BROWSER_SESSION:?}
case "$session" in */endora-browser.*) ;; *) exit 1;; esac
mode=$1
url=$2
bundle=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
mkdir "$session/bin" "$session/home"
cat > "$session/openbox.xml" <<'XML'
<?xml version="1.0"?>
<openbox_config xmlns="http://openbox.org/3.4/rc">
  <keyboard><keybind key="C-q"><action name="Exit"/></keybind></keyboard>
  <mouse><context name="Titlebar"><mousebind button="Left" action="Drag"><action name="Move"/></mousebind></context></mouse>
  <applications><application class="*"><maximized>yes</maximized></application></applications>
</openbox_config>
XML
openbox --config-file "$session/openbox.xml" >/dev/null 2>&1 &
wm=$!
browser=
login=
cleanup() {
    test -z "$browser" || kill "$browser" 2>/dev/null || true
    test -z "$login" || kill "$login" 2>/dev/null || true
    kill "$wm" 2>/dev/null || true
    wait 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
cat > "$session/bin/xdg-open" <<'SH'
#!/bin/sh
exec chromium-browser --user-data-dir="$ENDORA_BROWSER_SESSION/profile" --no-first-run --disable-sync --start-maximized "$1"
SH
chmod 700 "$session/bin/xdg-open"
if test "$mode" = auth; then
    # Capture the token rather than displaying it; the caller seals it immediately.
    HOME="$session/home" PATH="$session/bin:$PATH" cloudflared access login --app "$url" > "$session/access-token" 2> "$session/access-errors" &
    login=$!
elif test "$mode" = portal; then
    ENDORA_PORTAL_URL="$url" sh "$bundle/portal-browser.sh" >/dev/null 2>&1 &
    browser=$!
else
    "$session/bin/xdg-open" "$url" >/dev/null 2>&1 &
    browser=$!
fi
while kill -0 "$wm" 2>/dev/null; do
    if test -n "$browser" && ! kill -0 "$browser" 2>/dev/null; then
        wait "$browser" || exit 1
        break
    fi
    if test -n "$login" && ! kill -0 "$login" 2>/dev/null; then
        if wait "$login"; then
            # Parent directs stdout to a private pipe; never to a display/log.
            cat "$session/access-token"
        else exit 1; fi
        break
    fi
    sleep 0.2
done
