#!/usr/bin/env sh
# How big Endora's code is, by area: product code apart from tests, so growth is seen the day
# it happens (the owner, 31 Oct 2026: "keep tabs on the code base").
#
#   code-size.sh            Print product and test lines per area, and the scenario count.
#
# Lines are cloc's code lines (no comments or blanks). Product code leaves out test files
# (`*_tests.rs`, `tests.rs`, `tests/`, `features/`); a Rust file's own `#[cfg(test)]` module
# still counts as product, so the figures are an upper bound. Reads only; needs cloc.
set -eu
cd "$(dirname "$0")/.."
command -v cloc >/dev/null 2>&1 || { echo "code-size: cloc is not installed" >&2; exit 1; }

lines() { # $1 dir, then cloc filters
    dir=$1; shift
    [ -d "$dir" ] || { echo 0; return; }
    cloc "$dir" --quiet --include-lang=Rust,JavaScript "$@" 2>/dev/null |
        awk '/^SUM:/{print $5; f=1} END{if(!f) print 0}'
}

total_p=0
total_t=0
printf '%-24s %9s %9s\n' area product tests
for area in domains/team app/node app/hq app/worker app/runner domains/capabilities \
    domains/platform crates shared; do
    p=$(lines "$area" --exclude-dir=tests,features --not-match-f='(_tests\.rs|tests\.rs)$')
    a=$(lines "$area")
    t=$((a - p))
    total_p=$((total_p + p))
    total_t=$((total_t + t))
    printf '%-24s %9s %9s\n' "$area" "$p" "$t"
done
printf '%-24s %9s %9s\n' "all of the above" "$total_p" "$total_t"
printf 'scenarios: %s features\n' "$(find domains app -name '*.feature' | wc -l | tr -d ' ')"
