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
formats DSR/CPR, DECRPM, kitty keyboard and OSC 10/11 replies itself; the adapter only routes them. It also answers queries the Zig layer
never answers (DA2/DA3, XTVERSION, DECRQSS, XTGETTCAP, OSC 4/12/21 colour
queries, OSC 5522, colour-scheme/visibility DSR). `effects::route_reply` keeps
only the reply shapes `wasm_api.zig`'s `ResponseHandler` produces and splits
a multi-query OSC colour reply into one queue entry per answer, as the Zig
layer does. CPR is re-encoded from the absolute cursor because `rustty-vt`
reports it relative to the scrolling region under origin mode (DECOM) and the
Zig layer does not.

## Kitty graphics

Kitty graphics: not supported by the Rust build (no PNG/zlib on
wasm32-unknown-unknown); `printf` image demo is disabled for this engine.

Details (Task 5 decision gate, outcome B): the build with the `kitty-graphics`
feature *compiles* and has zero imports, but `rustty-vt` forces
`options::KITTY_GRAPHICS = false` on freestanding targets
(`target_os = "none"` and `wasm32-unknown-unknown`, mirroring the reference's
`Options.kittyGraphics` override: no timestamps, no wuffs PNG/zlib there). The
APC parser then routes every `ESC _ G` command to `Ignore`, the image storage
limit is never set, and the zlib arm is `unreachable!()`. Probed on the wasm:
a direct `f=24` transmission (`a=T,i=7`) and an `a=q` query get no reply,
while the same bytes on a host build get `ESC _ G i=7;OK ESC \`. Even direct
RGB/RGBA is therefore unavailable, not just PNG/compressed payloads.

So the feature is off (host builds too, so `cargo test` describes the shipped
engine) and the `graphics_*` exports are omitted. wterm's `GhosttyExports`
marks all of them optional; `GhosttyCore.getGraphicsState()` then reports no
images. `tools/audit-wasm.mjs` fails if any `graphics*` export reappears
(`NOT_EXPORTED`). Image commands are swallowed cleanly: no reply, no panic, no
effect on surrounding text, and file / temp-file / shared-memory media are never
executed. The Zig layer additionally caps an APC at 48 MiB and counts
rejections in `graphics_rejected`; here an APC of any size is discarded byte by
byte by the stream's `Ignore` state without buffering (tested at 50 MiB).

## Known gaps

Differences from the Zig layer that `rustty-vt` (read-only here) offers no
hook to close. Task 6 lists them in `harness/known-diffs.json`.

- **Kitty graphics.** Unsupported (see "Kitty graphics" above): no
  `graphics_*` exports, and kitty graphics commands get no `ESC _ G … OK`
  reply where the Zig layer answers.

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
