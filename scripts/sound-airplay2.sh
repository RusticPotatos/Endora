#!/bin/sh
# endora-sound's AirPlay 2 door (ADR 0128): shairport-sync built for AirPlay 2, with the nqptp
# clock it needs, in a container of its own on an address of its own on the LAN (a macvlan
# network), writing what a device casts into the pipe the hub reads.
#
# - Its own address, because nqptp needs UDP 319/320 to itself and the senders' airptpd holds
#   them on the host. Nothing else connects the two containers: only the pipes volume.
# - The doors to run are the hub's list, the same one the senders' container reads for its
#   classic doors (a pipe's file name, a tab, the name the owner's devices show). With
#   SOUND_AIRPLAY2=1 the senders run none of them and this runs them all, one AirPlay 2
#   receiver each, sharing one nqptp, each on a clock of its own.
# - Its own avahi, announcing the door with the door's own address: on its own address
#   nothing else answers for it, unlike the senders' container on the host's.
# - SOUND_AIRPLAY2_LATENCY (seconds, default 0.5): how early the door hands its sound to the
#   hub, to make up for the mix and the speaker after it, so an Apple TV's picture and the
#   speaker's sound line up. The owner tunes it (docs/homelab.md).
# - `render` writes the settings and makes the pipes, and starts nothing (its tests use it).
# - If anything stops, the container stops, and Docker starts it again whole.
set -eu

PIPES=${SOUND_PIPES:-/pipes}
RUN=${SOUND_RUN:-/run}
RECEIVERS="$PIPES/airplay-receivers.txt"
LATENCY=${SOUND_AIRPLAY2_LATENCY-0.5}
HOST=${SOUND_AIRPLAY2_HOSTNAME:-endora-mixer}
TAB=$(printf '\t')

fail() {
  echo "sound-airplay2: $*" >&2
  exit 2
}

# The setting shairport-sync takes: the lead, negative, so its sound comes out that much early.
# At most 2 s: an Apple TV's stream does not arrive far enough ahead for more, and shairport-sync
# drops an offset it cannot meet without saying so where anyone would look.
latency_offset() {
  case "$LATENCY" in
    "" | *[!0-9.]* | .* | *. | *.*.*) fail "SOUND_AIRPLAY2_LATENCY must be seconds from 0 to 2, like 0.5; got '$LATENCY'" ;;
  esac
  awk -v s="$LATENCY" 'BEGIN { exit !(s + 0 <= 2) }' \
    || fail "SOUND_AIRPLAY2_LATENCY must be seconds from 0 to 2, like 0.5; got '$LATENCY'"
  if awk -v s="$LATENCY" 'BEGIN { exit !(s + 0 == 0) }'; then
    echo "0.0"
    return
  fi
  echo "-$LATENCY"
}

# A name on the LAN (mDNS): letters, digits and hyphens, not first.
check_host() {
  case "$HOST" in
    "" | -* | *[!A-Za-z0-9-]*) fail "SOUND_AIRPLAY2_HOSTNAME must be letters, digits and hyphens, like endora-mixer; got '$HOST'" ;;
  esac
  [ "${#HOST}" -le 63 ] || fail "SOUND_AIRPLAY2_HOSTNAME is longer than 63 characters"
}

# The hub's list, only the lines that name a plain file in the pipes folder and a name.
receivers() {
  [ -f "$RECEIVERS" ] || return 0
  while IFS="$TAB" read -r pipe name; do
    [ -n "$pipe" ] && [ -n "$name" ] || continue
    case "$pipe" in */* | .*) continue ;; esac
    printf '%s\t%s\n' "$pipe" "$name"
  done < "$RECEIVERS"
}
receivers_now() { cksum < "$RECEIVERS" 2>/dev/null || echo none; }

# Made here before the receiver starts: it runs as its own user, which cannot make files in
# the pipes folder. A pipe already there is kept, because the hub may hold it open.
make_pipe() {
  [ -p "$1" ] && return 0
  rm -f "$1"
  mkfifo -m 0666 "$1"
}

quote() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

render_receiver() {
  r=$1 pipe=$2 name=$3 offset=$4
  cat > "$RUN/shairport-$r.conf" <<EOF
general = {
	name = "$(quote "$name")";
	service_type = "airplay2";
	output_backend = "pipe";
	mdns_backend = "avahi";
	port = $((7000 + r));
	udp_port_base = $((6001 + r * 10));
	udp_port_range = 10;
	nqptp_shared_memory_interface_name = "/nqptp-door-$r";
	audio_backend_latency_offset_in_seconds = $offset;
};
pipe = {
	name = "$PIPES/$pipe";
	output_rate = 44100;
	output_format = "S16_LE";
	output_channels = 2;
};
EOF
}

# Its own host name and its own address: on the macvlan network the door's address is its
# own, so avahi announces it (unlike the senders' container on the host's network).
render_avahi() {
  cat > "$RUN/avahi-daemon.conf" <<EOF
[server]
host-name=$HOST
use-ipv4=yes
use-ipv6=no
${1:+allow-interfaces=$1}
[wide-area]
enable-wide-area=no
[publish]
publish-addresses=yes
publish-hinfo=no
publish-workstation=no
EOF
}

render() {
  offset=$(latency_offset)
  check_host
  lan=${SOUND_AIRPLAY2_INTERFACE:-$(ip route show default 2>/dev/null | awk '{ for (i = 1; i < NF; i++) if ($i == "dev") { print $(i + 1); exit } }' || true)}
  mkdir -p "$RUN"
  rm -f "$RUN"/shairport-*.conf
  render_avahi "$lan"
  r=0
  list=$(receivers)
  if [ -n "$list" ]; then
    while IFS="$TAB" read -r pipe name; do
      make_pipe "$PIPES/$pipe"
      render_receiver "$r" "$pipe" "$name" "$offset"
      r=$((r + 1))
    done <<EOF
$list
EOF
  fi
  COUNT=$r
  if [ "$COUNT" -eq 1 ]; then
    echo "sound-airplay2: 1 AirPlay 2 receiver, handing its sound over ${LATENCY}s early"
  else
    echo "sound-airplay2: $COUNT AirPlay 2 receivers, handing their sound over ${LATENCY}s early"
  fi
}

if [ "${1:-run}" = render ]; then
  render
  exit 0
fi

render

mkdir -p /run/dbus /run/avahi-daemon
rm -f /run/dbus/dbus.pid /run/dbus/pid /run/avahi-daemon/pid
chown avahi:avahi /run/avahi-daemon
dbus-daemon --system
avahi-daemon --daemonize --no-chroot -f "$RUN/avahi-daemon.conf"

# The one clock every receiver here shares, each by its own name (/nqptp-door-<n>). As root:
# it binds UDP 319/320 and makes the shared memory the receivers read.
nqptp &
PIDS="$!"
sleep 1

r=0
while [ "$r" -lt "$COUNT" ]; do
  su-exec shairport shairport-sync -c "$RUN/shairport-$r.conf" &
  PIDS="$PIDS $!"
  r=$((r + 1))
done

SEEN=$(receivers_now)
while :; do
  if [ "$(receivers_now)" != "$SEEN" ]; then
    echo "sound-airplay2: the hub's doors changed; starting again to read them"
    kill $PIDS 2>/dev/null || true
    exit 0
  fi
  for pid in $PIDS; do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "sound-airplay2: a process ($pid) stopped; stopping the rest so Docker starts them again" >&2
      kill $PIDS 2>/dev/null || true
      exit 1
    fi
  done
  sleep 5
done
