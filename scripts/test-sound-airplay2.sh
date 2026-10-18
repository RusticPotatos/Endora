#!/usr/bin/env sh
# Tests for scripts/sound-airplay2.sh, the AirPlay 2 door (ADR 0128): what it writes for
# shairport-sync and avahi, and the pipe it hands the hub. Only its `render` step runs here,
# into a scratch folder, so it needs no daemon, no network and no root.
#
#   sh scripts/test-sound-airplay2.sh
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT="$HERE/sound-airplay2.sh"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/sound-airplay2-test.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

FAILED=0
expect() {
  # $1 what, $2 the status of the check (0 is a pass)
  if [ "$2" -eq 0 ]; then
    echo "  ok    $1"
  else
    echo "  FAIL  $1"
    FAILED=1
  fi
}

CASE=0
fresh() {
  CASE=$((CASE + 1))
  T="$WORK/case-$CASE"
  mkdir -p "$T/pipes" "$T/run"
}

# render, with the scratch folders and any extra NAME=value settings given.
render() {
  OUT=$(env SOUND_PIPES="$T/pipes" SOUND_RUN="$T/run" "$@" sh "$SCRIPT" render 2>&1)
  STATUS=$?
}

has() { grep -qF -- "$2" "$1"; }

TAB=$(printf '\t')

echo "the door's shairport-sync settings"

fresh
printf 'airplay-6.pcm\tEndora Mixer\n' >"$T/pipes/airplay-receivers.txt"
render
C="$T/run/shairport-0.conf"
expect "renders one receiver for the one door the hub lists" "$([ $STATUS -eq 0 ] && [ -f "$C" ] && [ ! -e "$T/run/shairport-1.conf" ]; echo $?)"
expect "  keeps the hub's name for it" "$(has "$C" 'name = "Endora Mixer";'; echo $?)"
expect "  is AirPlay 2, never a silent fall back to classic" "$(has "$C" 'service_type = "airplay2";'; echo $?)"
expect "  on AirPlay 2's own port" "$(has "$C" 'port = 7000;'; echo $?)"
expect "  writes into the pipe the hub reads" "$(has "$C" "name = \"$T/pipes/airplay-6.pcm\";"; echo $?)"
expect "  in the hub's format: 44.1 kHz" "$(has "$C" 'output_rate = 44100;'; echo $?)"
expect "  16-bit little-endian" "$(has "$C" 'output_format = "S16_LE";'; echo $?)"
expect "  stereo" "$(has "$C" 'output_channels = 2;'; echo $?)"
expect "  through the pipe back end and avahi" "$(has "$C" 'output_backend = "pipe";' && has "$C" 'mdns_backend = "avahi";'; echo $?)"
expect "  hands its sound over half a second early by default, for what comes after it" "$(has "$C" 'audio_backend_latency_offset_in_seconds = -0.5;'; echo $?)"
expect "  says how many receivers it rendered" "$(printf '%s' "$OUT" | grep -q '1 AirPlay 2 receiver'; echo $?)"

fresh
printf 'airplay-6.pcm\tEndora Mixer\n' >"$T/pipes/airplay-receivers.txt"
render SOUND_AIRPLAY2_LATENCY=0.25
expect "the lead is the owner's to tune" "$(has "$T/run/shairport-0.conf" 'audio_backend_latency_offset_in_seconds = -0.25;'; echo $?)"

fresh
printf 'airplay-6.pcm\tEndora Mixer\n' >"$T/pipes/airplay-receivers.txt"
render SOUND_AIRPLAY2_LATENCY=0
expect "  and can be none at all" "$(has "$T/run/shairport-0.conf" 'audio_backend_latency_offset_in_seconds = 0.0;'; echo $?)"

for bad in abc -0.5 2.5 1e3 '0.5; name = "x"' ''; do
  fresh
  printf 'airplay-6.pcm\tEndora Mixer\n' >"$T/pipes/airplay-receivers.txt"
  render SOUND_AIRPLAY2_LATENCY="$bad"
  expect "refuses a lead of '$bad', and renders nothing" "$([ $STATUS -ne 0 ] && [ ! -e "$T/run/shairport-0.conf" ] && printf '%s' "$OUT" | grep -q 'SOUND_AIRPLAY2_LATENCY'; echo $?)"
done

fresh
printf 'airplay-6.pcm\tThe "Den" \\ TV\n' >"$T/pipes/airplay-receivers.txt"
render
expect "a quote or a backslash in a name cannot end the setting early" "$(has "$T/run/shairport-0.conf" 'name = "The \"Den\" \\ TV";'; echo $?)"

fresh
{
  printf 'airplay-6.pcm\tEndora Mixer\n'
  printf 'airplay-iphone.pcm\tEndora Mixer (iPhone)\n'
} >"$T/pipes/airplay-receivers.txt"
render
A="$T/run/shairport-0.conf"
B="$T/run/shairport-1.conf"
expect "two doors get two receivers" "$([ -f "$A" ] && [ -f "$B" ]; echo $?)"
expect "  on ports of their own" "$(has "$A" 'port = 7000;' && has "$B" 'port = 7001;'; echo $?)"
expect "  with UDP ranges that do not overlap" "$(has "$A" 'udp_port_base = 6001;' && has "$B" 'udp_port_base = 6011;' && has "$A" 'udp_port_range = 10;'; echo $?)"
expect "  and a clock each from the one nqptp" "$(has "$A" 'nqptp_shared_memory_interface_name = "/nqptp-door-0";' && has "$B" 'nqptp_shared_memory_interface_name = "/nqptp-door-1";'; echo $?)"

fresh
{
  printf '../escape.pcm\tOut\n'
  printf 'sub/dir.pcm\tOut\n'
  printf '.hidden.pcm\tOut\n'
  printf 'no-name.pcm\t\n'
  printf '\tNo pipe\n'
  printf 'airplay-6.pcm\tEndora Mixer\n'
} >"$T/pipes/airplay-receivers.txt"
render
expect "only a plain file name in the pipes folder is ever written to" "$([ $STATUS -eq 0 ] && has "$T/run/shairport-0.conf" 'Endora Mixer' && [ ! -e "$T/run/shairport-1.conf" ] && [ ! -e "$T/escape.pcm" ] && [ ! -e "$T/pipes/sub" ]; echo $?)"

fresh
render
expect "no list from the hub yet: nothing to run, and no failure" "$([ $STATUS -eq 0 ] && [ ! -e "$T/run/shairport-0.conf" ] && printf '%s' "$OUT" | grep -q '0 AirPlay 2 receivers'; echo $?)"

echo "the pipe handed to the hub"

fresh
printf 'airplay-6.pcm\tEndora Mixer\n' >"$T/pipes/airplay-receivers.txt"
render
P="$T/pipes/airplay-6.pcm"
expect "makes the door's pipe before the receiver starts" "$([ -p "$P" ]; echo $?)"
expect "  that anyone may write: the receiver runs as its own user" "$([ "$(ls -l "$P" | cut -c1-10)" = prw-rw-rw- ]; echo $?)"

fresh
printf 'airplay-6.pcm\tEndora Mixer\n' >"$T/pipes/airplay-receivers.txt"
printf 'stale' >"$T/pipes/airplay-6.pcm"
render
expect "replaces a plain file left where the pipe goes" "$([ -p "$T/pipes/airplay-6.pcm" ]; echo $?)"

fresh
printf 'airplay-6.pcm\tEndora Mixer\n' >"$T/pipes/airplay-receivers.txt"
mkfifo -m 0666 "$T/pipes/airplay-6.pcm"
before=$(ls -i "$T/pipes/airplay-6.pcm" | awk '{print $1}')
render
after=$(ls -i "$T/pipes/airplay-6.pcm" | awk '{print $1}')
expect "keeps a pipe the hub already holds open, never a new one under it" "$([ "$before" = "$after" ]; echo $?)"

# What crosses: bytes written into the pipe by one process come out of the other, as the
# receiver's sound reaches the hub (two containers, one volume, one pipe).
fresh
printf 'airplay-6.pcm\tEndora Mixer\n' >"$T/pipes/airplay-receivers.txt"
render
cat "$T/pipes/airplay-6.pcm" >"$T/heard" &
reader=$!
printf 'PCM frames' >"$T/pipes/airplay-6.pcm"
wait "$reader"
expect "  and what the door writes is what the hub reads" "$([ "$(cat "$T/heard")" = 'PCM frames' ]; echo $?)"

echo "its own address on the LAN, announced by its own avahi"

fresh
render SOUND_AIRPLAY2_INTERFACE=eth0
V="$T/run/avahi-daemon.conf"
expect "announces under the door's own host name" "$(has "$V" 'host-name=endora-mixer'; echo $?)"
expect "  with its own address, which nothing else answers for" "$(has "$V" 'publish-addresses=yes'; echo $?)"
expect "  only on the LAN interface" "$(has "$V" 'allow-interfaces=eth0'; echo $?)"
expect "  IPv4 only, as the senders" "$(has "$V" 'use-ipv6=no'; echo $?)"

fresh
render SOUND_AIRPLAY2_HOSTNAME=den-door
expect "the host name is the owner's to choose" "$(has "$T/run/avahi-daemon.conf" 'host-name=den-door'; echo $?)"

for bad in 'two words' 'dots.local' '-lead' "$(printf 'a\nb')"; do
  fresh
  render SOUND_AIRPLAY2_HOSTNAME="$bad"
  expect "refuses the host name '$(printf '%s' "$bad" | tr '\n' ' ')'" "$([ $STATUS -ne 0 ] && printf '%s' "$OUT" | grep -q 'SOUND_AIRPLAY2_HOSTNAME'; echo $?)"
done

echo "the senders' container leaves the doors to it"

S="$HERE/sound-senders.sh"
expect "the senders run no receivers when SOUND_AIRPLAY2=1" "$(grep -q 'SOUND_AIRPLAY2:-0' "$S"; echo $?)"
expect "  and do not restart their senders when only the receivers change" "$(grep -q 'receivers_now() {' "$S" && sed -n '/^receivers_now() {/,/^}/p' "$S" | grep -q '"$AIRPLAY2" = 1'; echo $?)"

[ "$FAILED" -eq 0 ] && echo "sound-airplay2: every check passed"
exit "$FAILED"
