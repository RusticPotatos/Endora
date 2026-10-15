#!/usr/bin/env sh
# Tests for scripts/data-dir.sh against a fake `docker`, so they need no daemon and touch
# nothing real. The fake emulates only the handful of calls the script makes — and FAILS any
# `docker run` that lacks `--pull never`, so that safety flag is enforced here, not just
# present in the script.
#
#   sh scripts/test-data-dir.sh
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT="$HERE/data-dir.sh"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/data-dir-test.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

FAKEBIN="$WORK/bin"
mkdir -p "$FAKEBIN"
cat >"$FAKEBIN/docker" <<'FAKE'
#!/usr/bin/env sh
# A fake docker. State comes from the environment: FAKE_IMAGE (1/0), FAKE_VOLUME_DIR (a
# directory standing in for the named volume, or unset for "no such volume"), FAKE_RUNNING.
[ "${1:-}" = "--context" ] && shift 2
cmd="$1"
shift
echo "$cmd $*" >>"$FAKE_LOG"
case "$cmd" in
  image) [ "${FAKE_IMAGE:-1}" = "1" ] ;;
  volume) [ -n "${FAKE_VOLUME_DIR:-}" ] && [ -d "$FAKE_VOLUME_DIR" ] ;;
  ps) [ "${FAKE_RUNNING:-0}" = "1" ] && echo abc123; true ;;
  run)
    case " $* " in
      *" --pull never "*) ;;
      *)
        # Recorded where the final assertion can see it, and not only on stderr: a
        # violation the script swallowed would otherwise leave no trace.
        echo "docker run without --pull never: $*" >>"$FAKE_VIOLATIONS"
        echo "FAKE: docker run without --pull never" >&2
        exit 99
        ;;
    esac
    entry=""; vols=""; mnt=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --entrypoint) entry="$2"; shift 2 ;;
        -v) vols="$vols $2"; shift 2 ;;
        --mount) mnt="$2"; shift 2 ;;
        --pull) shift 2 ;;
        --rm|-i) shift ;;
        *) break ;;
      esac
    done
    shift # the image
    if [ "$entry" = "test" ]; then
      if [ -n "$mnt" ]; then
        src=${mnt#*source=}; src=${src%%,*}
        [ -d "$src" ] || exit 125   # docker refuses a bind mount whose source is missing
        dir="$src"
      else
        dir="${FAKE_VOLUME_DIR:-}"
      fi
      test -s "$dir/endora.db"
    else
      dest=""
      for v in $vols; do case "$v" in *:/to) dest="${v%:/to}" ;; esac; done
      mkdir -p "$dest"   # a bind mount of a missing host path is created by docker
      SRC="${FAKE_VOLUME_DIR:-}" DST="$dest" sh -s
    fi
    ;;
  *) echo "FAKE: unexpected docker $cmd" >&2; exit 98 ;;
esac
FAKE
chmod +x "$FAKEBIN/docker"
: >"$WORK/violations"

# The container image is Debian, which has sha256sum; macOS has `shasum -a 256` instead, with
# the same output format. Shimmed only where it is missing, so the script under test is
# exercised unchanged.
if ! command -v sha256sum >/dev/null 2>&1; then
  printf '#!/usr/bin/env sh\nexec shasum -a 256 "$@"\n' >"$FAKEBIN/sha256sum"
  chmod +x "$FAKEBIN/sha256sum"
fi

# A `cp` that damages the copy it just made, to prove the verification can fail.
BADBIN="$WORK/badbin"
mkdir -p "$BADBIN"
cat >"$BADBIN/cp" <<'BADCP'
#!/usr/bin/env sh
/bin/cp "$@" || exit $?
for last; do :; done
echo corruption >>"${last%/}/endora.db"
BADCP
chmod +x "$BADBIN/cp"

PASS=0
FAIL=0
n=0
ok() { PASS=$((PASS + 1)); printf '  ok    %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL  %s\n' "$1"; }
expect() { # expect "description" <status 0|1 meaning pass|fail>
  if [ "$2" -eq 0 ]; then ok "$1"; else bad "$1"; fi
}

# New isolated world per test: a volume dir with a database, an empty host dir.
fresh() {
  n=$((n + 1))
  T="$WORK/t$n"
  VOL="$T/volume"
  DIR="$T/host"
  mkdir -p "$VOL/sub" "$DIR"
  printf 'SQLite format 3 pretend database' >"$VOL/endora.db"
  printf 'cert' >"$VOL/tls-cert.pem"
  printf 'nested' >"$VOL/sub/file.txt"
  : >"$T/log"
}

# run_script <subcommand> [VAR=value...] ; sets OUT, STATUS. Defaults: image present, volume present.
run_script() {
  sub="$1"
  shift
  OUT=$(env PATH="${USE_PATH:-$FAKEBIN}:$PATH" FAKE_LOG="$T/log" FAKE_VIOLATIONS="$WORK/violations" \
    FAKE_IMAGE=1 FAKE_VOLUME_DIR="$VOL" \
    ENDORA_DATA_DIR="$DIR" "$@" sh "$SCRIPT" "$sub" 2>&1)
  STATUS=$?
}

echo "check (runs before every deploy)"

fresh
run_script check ENDORA_DATA_DIR=
expect "does nothing, and calls no docker, when ENDORA_DATA_DIR is unset" "$([ $STATUS -eq 0 ] && [ ! -s "$T/log" ]; echo $?)"

fresh
run_script check ENDORA_DATA_DIR=relative/path
expect "refuses a relative path" "$([ $STATUS -ne 0 ] && printf '%s' "$OUT" | grep -q 'absolute'; echo $?)"

fresh
run_script check FAKE_IMAGE=0
expect "allows a first deploy, when nothing is built on this host yet" "$([ $STATUS -eq 0 ]; echo $?)"

fresh
run_script check
expect "REFUSES an empty data dir while the volume still holds the data" "$([ $STATUS -ne 0 ]; echo $?)"
expect "  and says why in terms of what would go wrong" "$(printf '%s' "$OUT" | grep -q 'brand-new node'; echo $?)"
expect "  and names the way out" "$(printf '%s' "$OUT" | grep -q 'make migrate-data'; echo $?)"

fresh
printf 'db' >"$DIR/endora.db"
run_script check
expect "allows a data dir that already holds the database" "$([ $STATUS -eq 0 ]; echo $?)"

fresh
run_script check FAKE_VOLUME_DIR=
expect "allows a genuinely fresh node (no volume, empty dir)" "$([ $STATUS -eq 0 ]; echo $?)"
expect "  without ever mounting a volume that does not exist (docker would create it)" "$(! grep -q '^run.*-v ' "$T/log"; echo $?)"

fresh
run_script check ENDORA_DATA_DIR="$T/does-not-exist-yet"
expect "treats a data dir that does not exist yet as empty, and still refuses" "$([ $STATUS -ne 0 ]; echo $?)"

echo "migrate (one-time copy)"

fresh
run_script migrate FAKE_RUNNING=1
expect "refuses while the node is running" "$([ $STATUS -ne 0 ] && printf '%s' "$OUT" | grep -q 'Stop it first'; echo $?)"
expect "  and copies nothing" "$([ -z "$(ls -A "$DIR")" ]; echo $?)"

fresh
run_script migrate ENDORA_DATA_DIR=
expect "refuses with no ENDORA_DATA_DIR" "$([ $STATUS -ne 0 ]; echo $?)"

fresh
run_script migrate FAKE_VOLUME_DIR=
expect "says so plainly when the volume does not exist" "$([ $STATUS -ne 0 ] && printf '%s' "$OUT" | grep -q 'nothing to migrate'; echo $?)"

fresh
run_script migrate FAKE_IMAGE=0
expect "says so plainly when the image is not on the host" "$([ $STATUS -ne 0 ] && printf '%s' "$OUT" | grep -q 'not on this host'; echo $?)"

fresh
run_script migrate
expect "copies the volume and reports success" "$([ $STATUS -eq 0 ] && printf '%s' "$OUT" | grep -q 'every checksum identical'; echo $?)"
same=0
for f in endora.db tls-cert.pem sub/file.txt; do cmp -s "$VOL/$f" "$DIR/$f" || same=1; done
expect "  every file, including nested ones, is identical" "$same"
expect "  the source volume is untouched" "$([ "$(cat "$VOL/endora.db")" = 'SQLite format 3 pretend database' ]; echo $?)"

fresh
printf 'precious' >"$DIR/already-here"
run_script migrate
expect "refuses a destination that is not empty" "$([ $STATUS -ne 0 ]; echo $?)"
expect "  and leaves what was there alone" "$([ "$(cat "$DIR/already-here")" = precious ] && [ ! -e "$DIR/endora.db" ]; echo $?)"

fresh
rm "$VOL/endora.db"
run_script migrate
expect "refuses to copy a volume that has no database" "$([ $STATUS -ne 0 ]; echo $?)"

fresh
USE_PATH="$BADBIN:$FAKEBIN"
run_script migrate
unset USE_PATH
expect "DETECTS a copy that does not match, and fails" "$([ $STATUS -ne 0 ] && printf '%s' "$OUT" | grep -q 'DOES NOT MATCH'; echo $?)"

echo "safety"
expect "no docker run in this whole suite lacked --pull never" "$([ ! -s "$WORK/violations" ]; echo $?)"

printf '\n%s passed, %s failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
