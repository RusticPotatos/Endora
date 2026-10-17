#!/bin/sh
# The workbench's start (ring 1): a team member's disposable sandbox, where the worker gives the
# model a real shell. The root filesystem is read-only and /work (a tmpfs) is the only place to
# write, so the system's trust store cannot be updated in place: a bundle of the system's
# certificates and Endora's team CA is built in /work, and everything that speaks TLS is pointed
# at it. The team CA is what Endora's egress proxy signs the sites it reads with; it is trusted
# here and nowhere else.
#
# - git, curl and python honour HTTPS_PROXY (and its lower-case twin, which some of them read
#   instead), so the proxy the runner gives is the way out for all of them.
# - With no team CA mounted, the sandbox still starts: HTTPS through the proxy then fails, and
#   says so on stderr, which is the honest outcome.
set -eu

TEAM_CA=/etc/endora/team-ca.pem
SYSTEM=/etc/ssl/certs/ca-certificates.crt
BUNDLE=/work/.endora/ca-certificates.crt

mkdir -p "$(dirname "$BUNDLE")"
cat "$SYSTEM" > "$BUNDLE"
if [ -r "$TEAM_CA" ]; then
  printf '\n' >> "$BUNDLE"
  cat "$TEAM_CA" >> "$BUNDLE"
else
  echo "workbench: no team CA at $TEAM_CA; HTTPS through the proxy will not be trusted" >&2
fi

export SSL_CERT_FILE="$BUNDLE"
export CURL_CA_BUNDLE="$BUNDLE"
export GIT_SSL_CAINFO="$BUNDLE"
export REQUESTS_CA_BUNDLE="$BUNDLE"

# Both spellings of the proxy, whichever the runner gave.
if [ -n "${HTTPS_PROXY:-}" ] && [ -z "${https_proxy:-}" ]; then export https_proxy="$HTTPS_PROXY"; fi
if [ -n "${https_proxy:-}" ] && [ -z "${HTTPS_PROXY:-}" ]; then export HTTPS_PROXY="$https_proxy"; fi
if [ -n "${HTTP_PROXY:-}" ] && [ -z "${http_proxy:-}" ]; then export http_proxy="$HTTP_PROXY"; fi
if [ -n "${http_proxy:-}" ] && [ -z "${HTTP_PROXY:-}" ]; then export HTTP_PROXY="$http_proxy"; fi

cd /work
exec "$@"
