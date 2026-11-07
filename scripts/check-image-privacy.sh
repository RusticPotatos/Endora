#!/bin/sh
# Nothing local reaches Core's image (ADR 0147: it is published publicly on GHCR).
#
#   sh scripts/check-image-privacy.sh context [DIR]   what `docker build DIR` would send: no local
#                                                       config, keys, env files or databases
#   sh scripts/check-image-privacy.sh image IMAGE     the built image's files: the same, and none
#                                                       of ENDORA_PERSONAL_VALUES when it is set
#   sh scripts/check-image-privacy.sh --self-test     a planted file fails each check
#
# Needs Docker. The context check builds a throwaway image of the context alone, so it sees
# exactly what .dockerignore lets through.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)

# Files that are someone's own, by name. *.pem and *.key outside the system's CA folders.
FIND_LOCAL='find / -xdev \( -path /proc -o -path /sys -o -path /etc/ssl -o -path /usr/share/ca-certificates -o -path /usr/lib/ssl -o -path /usr/local/share/ca-certificates \) -prune -o -type f \( -name local.mk -o -name .env -o -name ".env.*" -o -name secrets.key -o -name "*.db" -o -name "*.db-wal" -o -name docker-compose.override.yml -o -name id_rsa -o -name id_ed25519 -o -name "*.pem" -o -name "*.key" \) ! -name "*.example" -print'

context() {
  dir=$1
  # Fails closed: no image built, or no listing read back, is a failure, never "clean".
  built=$(printf 'FROM busybox:1.37.0\nCOPY . /ctx\nRUN (%s 2>/dev/null | grep "^/ctx/" || true) > /found && echo listed > /done\nCMD ["sh", "-c", "cat /done /found"]\n' "$FIND_LOCAL" \
    | docker build -q -f - "$dir") || { echo "FAIL could not build the context of $dir"; return 1; }
  [ -n "$built" ] || { echo "FAIL could not build the context of $dir"; return 1; }
  listing=$(docker run --rm "$built") || { docker rmi -f "$built" >/dev/null 2>&1; echo "FAIL could not list the context of $dir"; return 1; }
  docker rmi -f "$built" >/dev/null 2>&1 || true
  [ "$(printf '%s\n' "$listing" | head -n 1)" = listed ] || { echo "FAIL could not list the context of $dir"; return 1; }
  found=$(printf '%s\n' "$listing" | tail -n +2)
  if [ -n "$found" ]; then
    echo "FAIL the build context of $dir carries local files:"; printf '  %s\n' $found; return 1
  fi
  echo "ok   the build context of $dir carries no local config, key, env file or database"
}

image() {
  img=$1; bad=0
  # Fails closed: the listing ends with a marker, so an image that could not be run is a failure.
  listing=$(docker run --rm --entrypoint sh "$img" -c "$FIND_LOCAL 2>/dev/null; echo listed") \
    || { echo "FAIL could not run $img"; return 1; }
  [ "$(printf '%s\n' "$listing" | tail -n 1)" = listed ] || { echo "FAIL could not list $img"; return 1; }
  found=$(printf '%s\n' "$listing" | sed '$d')
  if [ -n "$found" ]; then
    echo "FAIL $img carries local files:"; printf '  %s\n' $found; bad=1
  else
    echo "ok   $img carries no local config, key, env file or database"
  fi
  # The owner's own values, from local.mk on their machine; absent in CI, which is the point.
  if [ -n "${ENDORA_PERSONAL_VALUES:-}" ]; then
    old_ifs=$IFS; IFS=,
    for value in $ENDORA_PERSONAL_VALUES; do
      value=$(printf '%s' "$value" | sed 's/^ *//; s/ *$//')
      [ "${#value}" -gt 2 ] || continue
      if docker run --rm --entrypoint grep "$img" -rqsF -- "$value" /usr/local /etc /root 2>/dev/null; then
        echo "FAIL $img holds one of your own values"; bad=1
      fi
    done
    IFS=$old_ifs
    [ "$bad" -ne 0 ] || echo "ok   $img holds none of your own values"
  fi
  return $bad
}

self_test() {
  work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
  mkdir -p "$work/ctx/deploy/compose" "$work/ctx/app"
  # With no Docker, every check must fail rather than pass.
  if DOCKER_HOST=unix:///nonexistent.sock context "$work/ctx" >/dev/null 2>&1; then echo "FAIL self-test: no Docker passed"; return 1; fi
  cp "$here/.dockerignore" "$work/ctx/.dockerignore"
  echo 'ENDORA_PERSONAL_VALUES = John Doe' > "$work/ctx/local.mk"
  echo 'SOUND_TOKEN=placeholder' > "$work/ctx/deploy/compose/.env"
  echo 'placeholder' > "$work/ctx/app/server.key"
  echo 'kept' > "$work/ctx/app/main.rs"
  context "$work/ctx" >/dev/null || { echo "FAIL self-test: .dockerignore let a planted file through"; return 1; }
  : > "$work/ctx/.dockerignore"
  if context "$work/ctx" >/dev/null; then echo "FAIL self-test: a context with local.mk passed"; return 1; fi
  printf 'FROM busybox:1.37.0\nRUN echo x > /root/local.mk\n' | docker build -q -t endora-privacy-selftest:planted - >/dev/null
  if image endora-privacy-selftest:planted >/dev/null; then echo "FAIL self-test: an image with local.mk passed"; return 1; fi
  printf 'FROM busybox:1.37.0\nRUN echo x > /usr/local/notes\n' | docker build -q -t endora-privacy-selftest:clean - >/dev/null
  image endora-privacy-selftest:clean >/dev/null || { echo "FAIL self-test: a clean image failed"; return 1; }
  printf 'FROM busybox:1.37.0\nRUN echo "Jane Doe lives here" > /usr/local/notes\n' | docker build -q -t endora-privacy-selftest:value - >/dev/null
  if ENDORA_PERSONAL_VALUES="Jane Doe" image endora-privacy-selftest:value >/dev/null; then echo "FAIL self-test: an image holding a personal value passed"; return 1; fi
  docker rmi -f endora-privacy-selftest:planted endora-privacy-selftest:clean endora-privacy-selftest:value >/dev/null
  echo "ok   self-test: .dockerignore keeps planted files out; a planted file or value fails"
}

case "${1:-}" in
  context) context "${2:-$here}" ;;
  image) image "${2:?usage: check-image-privacy.sh image IMAGE}" ;;
  --self-test) self_test ;;
  *) echo "usage: check-image-privacy.sh context [DIR] | image IMAGE | --self-test" >&2; exit 2 ;;
esac
