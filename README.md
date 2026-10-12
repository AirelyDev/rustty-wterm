# rustty-wterm

wterm's `ghostty-vt.wasm` ABI, implemented on the Rustty VT engine (`rustty-vt`).

## Pins

- wterm reference commit: `488e9a7344e812e35a951f803618d7767cc4be5b`
- `rustty-vt` commit: `3e43a1d18` (path dependency `../rustty/crates/rustty-vt`, read-only)
- Toolchain: 1.93.1

## Build and test

```sh
cargo test
tools/build-wasm.sh   # release wasm + audit (zero imports, required exports)
```
