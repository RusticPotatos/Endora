#!/bin/sh
# Content, not timestamps, decides what the image build recompiles.
#
# Cargo decides whether one of our own crates is stale by comparing its sources' mtimes with
# the build it cached. In the image build those mtimes cannot be trusted: the target cache is
# shared by every checkout that deploys from this machine (the owner's, and each session's
# worktrees), so a file checked out an hour ago can be older than an artifact another
# checkout built a minute ago from different content, and cargo would ship that artifact.
# Touching every source fixed that, at the price of rebuilding every crate on every deploy.
#
# Instead this keeps, beside the cache, the hash of every file the last build was made from.
# `before` gives unchanged files an old mtime and changed or new ones the present, so cargo
# rebuilds exactly what changed; `after` records the new hashes once the build succeeded.
# The record also holds the binaries' mtimes: if they moved, a build that kept no record
# (an older Dockerfile, a failed run) changed the cache, the record is not trusted, and
# every file counts as changed.
#
#   fresh-sources.sh before <dir-of-record> <binary>...
#   fresh-sources.sh after  <dir-of-record> <binary>...
set -eu

step="$1"
record="$2/.endora-sources"
shift 2

hashes() {
    find . -path ./target -prune -o -type f -print | LC_ALL=C sort | xargs -d '\n' sha256sum
}

binaries() {
    for bin in "$@"; do
        if [ -e "$bin" ]; then
            echo "$(stat -c %Y "$bin") $bin"
        else
            echo "missing $bin"
        fi
    done
}

if [ "$step" = after ]; then
    mkdir -p "$(dirname "$record")"
    hashes > "$record.hashes.new"
    binaries "$@" > "$record.binaries.new"
    mv "$record.hashes.new" "$record.hashes"
    mv "$record.binaries.new" "$record.binaries"
    exit 0
fi

[ "$step" = before ] || { echo "fresh-sources: before or after, not $step" >&2; exit 2; }

hashes > /tmp/endora-sources.now
if [ ! -f "$record.hashes" ] || [ ! -f "$record.binaries" ] \
    || ! binaries "$@" | cmp -s - "$record.binaries"; then
    echo "fresh-sources: no record of the cached build to trust; every file counts as changed"
    find . -path ./target -prune -o -type f -exec touch {} +
    exit 0
fi

# Everything old first, then the present for what changed: a file is newer than the cached
# build exactly when its content is not what that build was made from.
find . -path ./target -prune -o -type f -exec touch -d @1 {} +
LC_ALL=C comm -13 "$record.hashes" /tmp/endora-sources.now | cut -c67- > /tmp/endora-sources.changed
echo "fresh-sources: $(wc -l < /tmp/endora-sources.changed) file(s) changed since the cached build"
if [ -s /tmp/endora-sources.changed ]; then
    xargs -d '\n' touch < /tmp/endora-sources.changed
fi
