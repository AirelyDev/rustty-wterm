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

## How effects reach the host

`rustty-vt` reports side effects through `stream_terminal::Effects`, plain `fn`
pointers with no user-data argument. `Core::write` points a module-level
"active sinks" pointer at its own sinks for the duration of the write
(`effects::ActiveGuard`; a `static mut` on single-threaded wasm32, a
`thread_local!` on host builds). Once `write_pty` is installed, `rustty-vt`
formats DSR/CPR, DECRPM, kitty keyboard, OSC 10/11 and kitty graphics replies
itself; the adapter only routes them. It also answers queries the Zig layer
never answers (DA2/DA3, XTVERSION, DECRQSS, XTGETTCAP, OSC 4/12/21 colour
queries, OSC 5522, colour-scheme/visibility DSR). `effects::route_reply` keeps
only the reply shapes `wasm_api.zig`'s `ResponseHandler` produces and splits
a multi-query OSC colour reply into one queue entry per answer, as the Zig
layer does. CPR is re-encoded from the absolute cursor because `rustty-vt`
reports it relative to the scrolling region under origin mode (DECOM) and the
Zig layer does not.

## Known gaps

Differences from the Zig layer that `rustty-vt` (read-only here) offers no
hook to close. Task 6 lists them in `harness/known-diffs.json`.

- **Tracked positions vs screen switches.** The Zig layer clears every tracked
  position when any single action changes the active screen. `rustty-vt` has
  no screen-switch effect, so the adapter compares the active screen before
  and after each `write`. A switch *and* return inside one `write` (e.g.
  `ESC[?1049h … ESC[?1049l` in one chunk) leaves primary-screen positions
  alive where the Zig layer would have released them.
- **OSC 52 selector.** `rustty-vt` maps every selector other than `s`/`p` to
  the standard clipboard before the `clipboard_write` effect, so selectors such
  as `0` or `q` are accepted; the Zig layer accepts only `c`.
- **OSC 52 base64.** The effect receives already-decoded bytes. `rustty-vt`
  tolerates missing `=` padding; the Zig layer (`std.base64.standard`)
  rejects it. (`sinks::ClipboardState::set` keeps the strict Zig rule for
  raw input.)
- **DECRQM for `?117`.** `rustty-vt` answers "permanently reset" (`;4$y`). The
  Zig layer answers from its own mode table (version skew; likely `;0$y`).
  Other modes known to one engine and not the other differ the same way
  (e.g. kitty paste events `?5522`).
