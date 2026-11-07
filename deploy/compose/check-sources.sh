#!/bin/sh
# Proves the bundle's vendored compose files are the pinned ones (ADR 0147): each file's
# SHA-256 against SOURCES.json, offline. With --remote, also fetches each from its repository at
# the pinned commit and compares, so a pin that no longer matches its source is caught.
#
#   sh deploy/compose/check-sources.sh [--remote]
#   sh deploy/compose/check-sources.sh --self-test
set -eu
here=$(cd "$(dirname "$0")" && pwd)

sha() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
field() { python3 -c "import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]][sys.argv[3]])" "$1" "$2" "$3"; }

# check DIR [--remote]: 0 when every pinned file in DIR matches, 1 otherwise.
check() {
  dir=$1; remote=${2:-}; bad=0
  for product in console audio; do
    file="$dir/$(field "$dir/SOURCES.json" "$product" path)"
    want=$(field "$dir/SOURCES.json" "$product" sha256)
    got=$(sha "$file")
    if [ "$got" != "$want" ]; then
      echo "FAIL $product: $file is not the pinned file (sha256 $got, pinned $want)"; bad=1; continue
    fi
    echo "ok   $product: $file matches its pin"
    if [ "$remote" = --remote ]; then
      repo=$(field "$dir/SOURCES.json" "$product" repo)
      commit=$(field "$dir/SOURCES.json" "$product" commit)
      source=$(field "$dir/SOURCES.json" "$product" source)
      fetched=$(mktemp)
      curl -fsSL "https://raw.githubusercontent.com/$repo/$commit/$source" -o "$fetched"
      if [ "$(sha "$fetched")" != "$want" ]; then
        echo "FAIL $product: $repo@$commit:$source differs from the pin"; bad=1
      else
        echo "ok   $product: matches $repo@$commit"
      fi
      rm -f "$fetched"
    fi
  done
  return $bad
}

if [ "${1:-}" = --self-test ]; then
  # A changed byte in a vendored file must fail the check, and the untouched copy must pass.
  copy=$(mktemp -d); trap 'rm -rf "$copy"' EXIT
  cp -R "$here/." "$copy/"
  check "$copy" >/dev/null || { echo "FAIL self-test: the untouched copy did not pass"; exit 1; }
  printf '\n# changed\n' >> "$copy/audio/compose.yaml"
  if check "$copy" >/dev/null; then echo "FAIL self-test: a changed file passed"; exit 1; fi
  echo "ok   self-test: a changed vendored file fails the check"
  exit 0
fi
check "$here" "${1:-}"
