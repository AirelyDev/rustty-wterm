#!/bin/sh
# Release wasm build, audited. Usage: tools/build-wasm.sh
set -e
cd "$(dirname "$0")/.."
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
cargo rustc --release --target wasm32-unknown-unknown -- -C link-arg=-zstack-size=131072
out=target/wasm32-unknown-unknown/release/rustty_wterm.wasm
node tools/audit-wasm.mjs "$out"
ls -l "$out"
