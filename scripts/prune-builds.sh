#!/usr/bin/env sh
# Keep the disk from filling with build output: of the Rust build folders (`target/`) in the
# git worktrees beside this repository, keep the most recently used and delete the rest.
#
#   prune-builds.sh [keep] [parent]   keep: how many to keep (default 15)
#                                     parent: the folder holding the worktrees (default: the
#                                     one holding this repository)
#
# Why it exists: on 31 Oct 2026 about 100 worktrees' build folders filled the disk (over
# 300 GB) and every build and test stopped. A build folder is cargo's own output and is made
# again by the next build, so deleting one loses no work.
#
# Only a `target/` folder inside a git worktree (a folder whose `.git` is a file) is deleted.
# A main checkout (a `.git` folder) is never touched, nor is any folder named in
# PRUNE_BUILDS_KEEP (space-separated worktree folder names, e.g. "Endora-gate").
set -eu
keep=${1:-15}
here=$(cd "$(dirname "$0")/.." && pwd)
parent=${2:-$(dirname "$here")}
spared=${PRUNE_BUILDS_KEEP:-}

case $keep in '' | *[!0-9]*) echo "prune-builds: keep must be a number" >&2; exit 2 ;; esac

list=$(mktemp)
trap 'rm -f "$list"' EXIT
for target in "$parent"/*/target; do
    [ -d "$target" ] || continue
    tree=$(dirname "$target")
    [ -f "$tree/.git" ] || continue
    name=$(basename "$tree")
    case " $spared " in *" $name "*) continue ;; esac
    # Most recently used first, by the folder's own time (cargo touches it on every build).
    printf '%s %s\n' "$(stat -f %m "$target" 2>/dev/null || stat -c %Y "$target")" "$target"
done | sort -rn >"$list"

total=$(wc -l <"$list" | tr -d ' ')
[ "$total" -gt "$keep" ] || { echo "prune-builds: $total build folders, none over $keep"; exit 0; }
tail -n +"$((keep + 1))" "$list" | while read -r _ target; do
    rm -rf "$target"
    echo "prune-builds: removed $target"
done
echo "prune-builds: kept $keep of $total"
