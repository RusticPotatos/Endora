#!/bin/sh
set -eu
umask 077
bundle=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
base="$HOME/.local/share/endora-terminal"
bin="$HOME/.local/bin"
for path in "$HOME/.local" "$HOME/.local/share" "$base" "$bin"; do
    test ! -L "$path" || { echo 'Refusing symlinked installation directory' >&2; exit 1; }
done
if test -L "$bin/endora"; then
    test "$(readlink "$bin/endora")" = "$bin/endora-terminal" || { echo 'Existing endora shortcut is unmanaged' >&2; exit 1; }
elif test -e "$bin/endora"; then
    echo 'Existing endora command is unmanaged' >&2; exit 1
fi
mkdir -p "$base" "$bin"
chmod 700 "$base"
mkdir "$base/.install-lock" || { echo 'Another install is running' >&2; exit 1; }
trap 'rm -rf "$base/.install-lock"' EXIT HUP INT TERM
replace_link() {
    case "$(uname -s)" in
        Linux) mv -Tf "$1" "$2";;
        Darwin) mv -fh "$1" "$2";;
        *) echo 'Unsupported installer OS' >&2; exit 1;;
    esac
}
version=$(cat "$bundle/VERSION")
case "$version" in ''|*[!0-9.]*|.*) echo 'Invalid release version' >&2; exit 1;; esac
for name in endora-terminal boot.sh launcher.sh install.sh uninstall.sh browser.sh browser-session.sh portal-browser.sh device-install.sh backlight-helper.sh desktop.sh desktop-watch.py text-browser.sh; do
    test -f "$bundle/$name" && test ! -L "$bundle/$name" || exit 1
done
if test "${1:-}" = rollback; then
    test -L "$base/previous" || { echo 'No previous release' >&2; exit 1; }
    previous=$(readlink "$base/previous")
    case "$previous" in releases/*) ;; *) exit 1;; esac
    test -x "$base/$previous/endora-terminal" || exit 1
    ln -s "$previous" "$base/.install-lock/current"
    replace_link "$base/.install-lock/current" "$base/current"
    echo 'Previous release restored'
    exit 0
fi
mkdir -p "$base/releases"
test ! -L "$base/releases" || exit 1
stage="$base/.install-lock/release"
mkdir "$stage"
for name in endora-terminal boot.sh launcher.sh install.sh uninstall.sh VERSION browser.sh browser-session.sh portal-browser.sh device-install.sh backlight-helper.sh desktop.sh desktop-watch.py text-browser.sh; do
    cp "$bundle/$name" "$stage/$name"
done
chmod 700 "$stage/endora-terminal" "$stage/boot.sh" "$stage/launcher.sh" "$stage/browser.sh" "$stage/browser-session.sh" "$stage/portal-browser.sh" "$stage/device-install.sh" "$stage/backlight-helper.sh" "$stage/desktop.sh" "$stage/desktop-watch.py" "$stage/text-browser.sh"
"$stage/endora-terminal" --help >/dev/null
dest="$base/releases/$version"
if test -e "$dest" || test -L "$dest"; then
    test ! -L "$dest" || exit 1
    for name in endora-terminal boot.sh launcher.sh install.sh uninstall.sh VERSION browser.sh browser-session.sh portal-browser.sh device-install.sh backlight-helper.sh desktop.sh desktop-watch.py text-browser.sh; do
        cmp -s "$stage/$name" "$dest/$name" || { echo 'Version already exists with different contents' >&2; exit 1; }
    done
else
    mv "$stage" "$dest"
fi
if test -L "$base/current"; then
    current=$(readlink "$base/current")
    case "$current" in releases/*) ;; *) exit 1;; esac
    if test "$current" != "releases/$version"; then
        ln -s "$current" "$base/.install-lock/previous"
        replace_link "$base/.install-lock/previous" "$base/previous"
    fi
elif test -e "$base/current"; then
    echo 'Refusing to replace unmanaged current path' >&2; exit 1
fi
ln -s "releases/$version" "$base/.install-lock/current"
replace_link "$base/.install-lock/current" "$base/current"
if test -e "$bin/endora-terminal" && test ! -L "$bin/endora-terminal"; then
    echo 'Refusing to replace unmanaged executable' >&2; exit 1
fi
ln -s "$base/current/endora-terminal" "$base/.install-lock/bin"
replace_link "$base/.install-lock/bin" "$bin/endora-terminal"
ln -s "$bin/endora-terminal" "$base/.install-lock/alias"
replace_link "$base/.install-lock/alias" "$bin/endora"
apps="$HOME/.local/share/applications"
if test ! -L "$apps"; then
    mkdir -p "$apps"
    desktop="$apps/endora-terminal.desktop"
    # Keep an existing launcher. Quote normal home paths for the desktop Exec parser.
    if test ! -e "$desktop" && test ! -L "$desktop"; then
        case "$HOME" in *'"'*|*'%'*|*'\'*|*'`'*|*'$'*) :;;
        *) printf '[Desktop Entry]\nType=Application\nName=Endora\nComment=Endora terminal client\nExec="%s/.local/bin/endora-terminal"\nTerminal=true\nCategories=System;Utility;\nIcon=utilities-terminal\nX-Endora-Managed=true\n' "$HOME" > "$desktop"; chmod 644 "$desktop";;
        esac
    fi
fi
echo 'Installed. Run endora setup, then endora doctor. An Endora application-menu launcher is available.'
