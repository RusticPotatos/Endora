#!/bin/sh
# endora-sound's AirPlay senders (ADR 0104): one shared airptpd and four stock OwnTone
# instances, each playing the pipe the hub writes for one speaker group.
#
# - On the host's network, because AirPlay speakers are found and reached over mDNS.
# - airptpd first: it is the one AirPlay 2 clock all four share. Without it only the first
#   instance would get PTP, and the others would fall back to NTP.
# - Each instance's API trusts only localhost; anyone else needs the password made here once,
#   which only the hub is given (its file is in a volume only the two of them mount).
# - If any of them stops, the container stops, and Docker starts it again whole.
set -eu

SENDERS=4
SECRET=/secret/password
# Every AirPlay speaker the hub knows, one a line, written by the hub.
NAMES=/pipes/airplay-speakers.txt

# OwnTone takes a speaker back by itself five seconds after another device takes it: from a
# Mac playing to the HomePod natively, say, which then falls back to its own speakers. Never:
# the hub lets a borrowed speaker go, and asks for it again only when it is free.
speaker_sections() {
  [ -f "$NAMES" ] || return 0
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    quoted=$(printf '%s' "$name" | sed 's/\\/\\\\/g; s/"/\\"/g')
    printf 'airplay "%s" {\n\treconnect = false\n}\n' "$quoted"
  done < "$NAMES"
}
names_now() { cksum < "$NAMES" 2>/dev/null || echo none; }

if [ ! -s "$SECRET" ]; then
  umask 077
  head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n' > "$SECRET"
fi
PASSWORD=$(cat "$SECRET")

# Discovery: OwnTone finds speakers through avahi, over dbus.
mkdir -p /run/dbus /run/avahi-daemon
rm -f /run/dbus/dbus.pid /run/dbus/pid /run/avahi-daemon/pid
chown avahi:avahi /run/avahi-daemon
dbus-daemon --system
avahi-daemon --daemonize --no-chroot

airptpd -f &
PIDS="$!"
sleep 1

n=1
while [ "$n" -le "$SENDERS" ]; do
  mkdir -p "/data/sender-$n/cache" "/pipes/sender-$n"
  chown -R owntone "/data/sender-$n"
  cat > "/run/owntone-$n.conf" <<EOF
general {
	uid = "owntone"
	db_path = "/data/sender-$n/songs3.db"
	db_backup_path = "/data/sender-$n/songs3.bak"
	cache_dir = "/data/sender-$n/cache"
	# In the foreground it logs to the console already (docker logs); a second copy of each
	# line would only double them, and its own user cannot open root's /dev/stderr anyway.
	logfile = "/dev/null"
	loglevel = "log"
	admin_password = "$PASSWORD"
	websocket_port = 371$n
	trusted_networks = { "localhost" }
	ipv6 = no
	speaker_autoselect = no
	# HomePods on HomePod OS 27 answer 403 to any AirPlay request whose User-Agent does not
	# start with "AirPlay/", which OwnTone's own does not (the fix ansible-nas carries too).
	user_agent = "AirPlay/999.0.0"
}
library {
	name = "Endora sender $n"
	port = 370$n
	directories = { "/pipes/sender-$n" }
	pipe_autostart = true
	pipe_sample_rate = 44100
	pipe_bits_per_sample = 16
}
audio {
	type = "disabled"
}
mpd {
	port = 0
}
EOF
  speaker_sections >> "/run/owntone-$n.conf"
  owntone -f -c "/run/owntone-$n.conf" &
  PIDS="$PIDS $!"
  n=$((n + 1))
done

echo "sound: airptpd and $SENDERS senders started, $(grep -c 'reconnect = false' /run/owntone-1.conf || true) speakers they will not take back by themselves"
SEEN=$(names_now)
while :; do
  if [ "$(names_now)" != "$SEEN" ]; then
    echo "sound: the hub's speakers changed; starting again to read them"
    kill $PIDS 2>/dev/null || true
    exit 0
  fi
  for pid in $PIDS; do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "sound: a sender process ($pid) stopped; stopping the rest so Docker starts them again" >&2
      kill $PIDS 2>/dev/null || true
      exit 1
    fi
  done
  sleep 5
done
