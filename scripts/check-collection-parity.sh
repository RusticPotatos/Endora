#!/bin/sh
# Parity between docker-compose.yml and the endora.homelab collection's `endora` role: every
# variable the compose file interpolates must be one the role renders into endora.env (its
# template names it) or one that matters only on the Mac, listed below; every Compose profile
# must be one the role can start. A new compose variable without a role variable fails here.
#   scripts/check-collection-parity.sh              check the repository
#   scripts/check-collection-parity.sh --self-test  prove the check fails on an unknown variable
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
role="$root/deploy/ansible/collection/roles/endora"

# Mac-only, kept small and explicit: ENDORA_BUILD stamps the image at build time (`make deploy`
# on the Mac); `bundled` is the runtime profile for a model baked into a local build.
mac_only_vars="ENDORA_BUILD"
mac_only_profiles="bundled"

check() {
  compose=$1
  status=0
  vars=$(grep -o '\${[A-Za-z_][A-Za-z_0-9]*' "$compose" | sed 's/^\${//' | sort -u)
  for name in $vars; do
    case " $mac_only_vars " in *" $name "*) continue ;; esac
    if grep -q "^$name=" "$role/templates/endora.env.j2"; then continue; fi
    echo "parity: $name is interpolated by $(basename "$compose") but the endora role does not render it" >&2
    echo "        add a role variable, render it in templates/endora.env.j2, test it in tests/render.yml" >&2
    status=1
  done
  profiles=$(grep -o 'profiles: *\[[^]]*\]' "$compose" | sed 's/.*\[//; s/\]//; s/[",]/ /g' | tr ' ' '\n' | grep . | sort -u)
  for name in $profiles; do
    case " $mac_only_profiles " in *" $name "*) continue ;; esac
    if grep -q "'$name'" "$role/defaults/main.yml"; then continue; fi
    echo "parity: profile $name is in $(basename "$compose") but endora_profiles never starts it" >&2
    status=1
  done
  return $status
}

if [ "${1:-}" = "--self-test" ]; then
  scratch=$(mktemp -d)
  trap 'rm -rf "$scratch"' EXIT HUP INT TERM
  cp "$root/docker-compose.yml" "$scratch/docker-compose.yml"
  printf '      ENDORA_PARITY_PROBE: "${ENDORA_PARITY_PROBE-}"\n  probe:\n    profiles: [probe]\n' >>"$scratch/docker-compose.yml"
  if check "$scratch/docker-compose.yml" 2>/dev/null; then
    echo "parity self-test: an unknown variable and profile were NOT caught" >&2
    exit 1
  fi
  echo "parity self-test: an unknown variable and profile are caught"
  exit 0
fi

check "$root/docker-compose.yml"
echo "collection parity: every compose variable and profile is declared by the endora role"
