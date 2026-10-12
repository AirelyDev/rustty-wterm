#!/bin/sh
# One-command check: format, lint, host tests, audited wasm build, and the
# differential harness against wterm's ghostty-vt.wasm. Exit 0 = all green.
set -e
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
[ -d harness/node_modules ] || (cd harness && npm ci)
# The corpus is generated; fail if the committed files are stale.
node harness/corpus/gen.mjs >/dev/null
git diff --exit-code --stat -- harness/corpus
tools/build-wasm.sh --diff
node harness/robustness.mjs target/wasm32-unknown-unknown/release/rustty_wterm.wasm
