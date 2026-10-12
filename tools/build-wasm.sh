#!/bin/sh
# Release wasm build, audited. Usage: tools/build-wasm.sh [--diff]
# --diff also runs the differential harness against ghostty-vt.wasm.
set -e
cd "$(dirname "$0")/.."
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
cargo rustc --release --target wasm32-unknown-unknown -- -C link-arg=-zstack-size=131072
out=target/wasm32-unknown-unknown/release/rustty_wterm.wasm
node tools/audit-wasm.mjs "$out"
ls -l "$out"
if [ "$1" = "--diff" ]; then
  [ -d harness/node_modules ] || (cd harness && npm ci)
  node harness/diff.mjs --rust "$out" --zig harness/node_modules/@wterm/ghostty/wasm/ghostty-vt.wasm
fi
