#!/bin/sh
# The full handoff gate (AGENTS.md "Before handoff") on a tree: $1, else this checkout. Logs go
# under .local/gate of that tree; it prints one line a check, the unit/scenario/step totals and
# the code size. Run it on a detached copy so a gate never races the working tree:
#   git -C ../Endora-gate fetch "$PWD" <branch> && git -C ../Endora-gate checkout --detach FETCH_HEAD
#   CARGO_TARGET_DIR=../Endora-gate/target sh scripts/gate.sh ../Endora-gate
cd "${1:-$(dirname "$0")/..}" || exit 1
L=.local/gate; mkdir -p $L
[ -f scripts/prune-builds.sh ] && PRUNE_BUILDS_KEEP="Endora-gate Endora-agent-collaboration" sh scripts/prune-builds.sh 15 "$HOME/repos" | tail -1
cargo fmt --all -- --check >$L/fmt.log 2>&1 && echo fmt-ok || { echo fmt-FAIL; exit 1; }
cargo clippy --workspace --all-targets --all-features -- -D warnings >$L/clippy.log 2>&1 && echo clippy-ok || { echo clippy-FAIL; tail -30 $L/clippy.log; exit 1; }
cargo test --workspace --all-features -- --test-threads=4 >$L/test.log 2>&1; r=$?
p=$(grep -o 'test result: [a-zA-Z]*\. [0-9]* passed' $L/test.log | awk '{s+=$4}END{print s}'); f=$(grep -o '[0-9]* failed' $L/test.log | awk '{s+=$1}END{print s}')
sc=$(grep -E '^[0-9]+ scenarios' $L/test.log | awk '{s+=$1}END{print s}'); st=$(grep -E '^[0-9]+ steps' $L/test.log | awk '{s+=$1}END{print s}')
echo "unit tests passed $p; scenarios $sc ($st steps); failed $f"; grep -n 'FAILED\|panicked' $L/test.log | head
[ $r = 0 ] && echo test-ok || exit 1
timeout 300 node scripts/check-console.mjs >$L/console.log 2>&1 && echo console-ok || { echo console-FAIL; tail $L/console.log; exit 1; }
node scripts/check-nothing-personal.mjs >$L/personal.log 2>&1 && echo personal-ok || { echo personal-FAIL; exit 1; }
sh scripts/check-collection-parity.sh >$L/parity.log 2>&1 && echo parity-ok || { echo parity-FAIL; exit 1; }
# The bundle's isolation check and its pins, run by name: a check missing from the manifest, a
# script that is not there, or a stub that prints nothing fails here, never shrinks the gate.
sh scripts/check-required.sh --require isolation --require sources --require sources-self-test --require required-self-test scripts/required-checks.txt >$L/required.log 2>&1; r=$?
sed 's/^/required: /' $L/required.log; [ $r = 0 ] && echo required-ok || { echo required-FAIL; exit 1; }
git diff --check && echo diff-ok
[ -f scripts/code-size.sh ] && sh scripts/code-size.sh | sed 's/^/size: /'
