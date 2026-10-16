#!/bin/sh
set -eu
# Avoid embedding the builder's personal home paths in panic/source-location strings.
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$HOME=/build"
# Inside Endora's workspace the build would land in the repository root's target/; keep it
# here, where package.sh looks for it.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}"
cargo zigbuild --locked --release --target aarch64-unknown-linux-musl
