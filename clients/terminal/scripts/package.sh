#!/bin/sh
set -eu
version=$(sed -n 's/^version = "\([0-9.]*\)"/\1/p' Cargo.toml | head -1)
target=aarch64-unknown-linux-musl
test -x "target/$target/release/endora-terminal"
python3 - "target/$target/release/endora-terminal" <<'PY'
import pathlib, sys
body = pathlib.Path(sys.argv[1]).read_bytes()
if str(pathlib.Path.home()).encode() in body:
    raise SystemExit('Release contains a local build path; use scripts/build-arm64.sh')
PY
mkdir -p dist
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
cp "target/$target/release/endora-terminal" "$stage/endora-terminal"
for file in install.sh boot.sh launcher.sh README.md browser.sh browser-session.sh portal-browser.sh device-install.sh desktop.sh text-browser.sh; do cp "$file" "$stage/$file"; done
cp scripts/uninstall.sh "$stage/uninstall.sh"
cp scripts/backlight-helper.sh "$stage/backlight-helper.sh"
cp scripts/desktop-watch.py "$stage/desktop-watch.py"
printf '%s\n' "$version" > "$stage/VERSION"
archive="endora-terminal-$version-aarch64-linux-musl.tar.gz"
python3 - "$stage" "dist/$archive" <<'PY'
import gzip, pathlib, sys, tarfile
stage = pathlib.Path(sys.argv[1])
with open(sys.argv[2], 'wb') as output:
    with gzip.GzipFile(filename='', fileobj=output, mode='wb', mtime=0) as zipped:
        with tarfile.open(fileobj=zipped, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            for name in ('endora-terminal', 'install.sh', 'boot.sh', 'launcher.sh', 'uninstall.sh', 'README.md', 'VERSION', 'browser.sh', 'browser-session.sh', 'portal-browser.sh', 'device-install.sh', 'backlight-helper.sh', 'desktop.sh', 'desktop-watch.py', 'text-browser.sh'):
                path = stage / name
                assert path.is_file() and not path.is_symlink()
                info = archive.gettarinfo(str(path), arcname=name)
                info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ''
                info.mode = 0o755 if name == 'endora-terminal' or name.endswith(('.sh', '.py')) else 0o644
                with path.open('rb') as source:
                    archive.addfile(info, source)
PY
(cd dist && shasum -a 256 "$archive" > SHA256SUMS)
