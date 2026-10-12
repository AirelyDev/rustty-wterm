# rustty-wterm

wterm's `ghostty-vt.wasm` ABI, implemented on the Rustty VT engine (`rustty-vt`).

## Pins

- wterm reference commit: `488e9a7344e812e35a951f803618d7767cc4be5b`
- `rustty-vt` commit: `3e43a1d18` (path dependency `../rustty/crates/rustty-vt`, read-only)
- Toolchain: 1.93.1

## Build and test

```sh
cargo test
tools/build-wasm.sh          # release wasm + audit (zero imports, required exports)
tools/build-wasm.sh --diff   # ... then the differential harness
tools/check.sh               # everything: fmt, clippy, tests, wasm build, harness
```

`tools/check.sh` runs `cargo fmt --check`, `cargo clippy --all-targets -- -D
warnings`, `cargo test`, regenerates the harness corpus (failing if the
committed files are stale), `tools/build-wasm.sh` and `node harness/diff.mjs`;
it exits 0 only when all pass.

## Differential harness

`harness/` feeds the same bytes to this build and to wterm's own
`ghostty-vt.wasm` (`@wterm/ghostty@0.5.4`, ghostty v1.3.1), both through
wterm's unmodified `GhosttyCore`, and compares every observable: cells,
scrollback, responses, cursor, title, cwd, bell, clipboard, shell state,
colour overrides, modes, wrap metadata, discarded rows, prompts and tracked
positions. Every difference is either fixed or listed with evidence in
`harness/known-diffs.json`. See `harness/README.md`.

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

## Discarded scrollback rows

`get_scrollback_discarded_count` reports the Zig patch's `discardedRows`
counter: rows of pages that `grow` pruned from the oldest end of the active
screen, zeroed by RIS, untouched by ED 3. `rustty-vt` has no such counter, so
`Core` observes prunes from outside (see `DiscardTracker` in `src/core.rs`): a
tracked pin on the primary screen's first row is marked garbage only by a
prune (or a reset, told apart by `page_serial_epoch`), and `Core::write` feeds
the stream in pieces of half a page of bytes so that at most one prune
happens between samples. The count is exact for line output (the harness and
`tests/scrollback.rs` check history + rows + discarded = lines written, in
one write or many); see the last known gap for where it can undercount.

## Known gaps

Differences from the Zig layer that `rustty-vt` (read-only here) offers no
hook to close. `harness/known-diffs.json` lists the ones the harness
observes (owner `rustty-vt`).

- **Kitty graphics.** Unsupported (see "Kitty graphics" above): no
  `graphics_*` exports, and kitty graphics commands get no `ESC _ G … OK`
  reply where the Zig layer answers. The harness skips `kitty-image-rgb`.
- **Tracked positions vs screen switches.** The Zig layer clears every tracked
  position when any single action changes the active screen. `rustty-vt` has
  no screen-switch effect, so the adapter compares the active screen before
  and after each `write`. A switch *and* return inside one `write` (e.g.
  `ESC[?1049h … ESC[?1049l` in one chunk) leaves primary-screen positions
  alive where the Zig layer would have released them (harness entry
  `tracked-alt-screen`, whole-write chunking only).
- **Discarded rows under multi-row bursts.** A second prune between two
  samples is missed. That needs a page of new rows (hundreds at 80 columns)
  from half a page of input bytes, which only REP (`CSI n b`) or a run of
  ED 2 scroll-clears on very wide or very short pages can produce.

## Findings

What the differential harness found (`harness/known-diffs.json` has the
evidence for each). No difference is a `rustty-vt` porting bug: apart from
the gaps above, every engine difference traces to an upstream ghostty commit
between wterm's v1.3.1 and Rustty's oracle `246f70287`, and `rustty-vt`
faithfully reproduces the newer behaviour.

- **Page size.** The page layout rewrite in ghostty `d2ff6d77a` changes how
  many rows fit in a page (at 80 columns: 589 rows in the Zig build, 673 in
  the Rust build), so a byte-limited scrollback keeps and discards different
  amounts of history. Both engines are self-consistent.
- **DECRQM.** v1.3.1 does not know modes 67 (DECBKM, added in `aea70a5f7`)
  or 117 (DECECM, reported permanently reset since `c092b2bcf`), and drops
  ANSI-form `CSI Ps $ p` entirely (answered since `9dc0d974e`).
- **OSC 52.** Newer ghostty normalises the selector to a clipboard location
  (`634ef7198`: `0`, `q`, … write the standard clipboard) and tolerates
  missing base64 padding (`bc2f7d7d2`); wterm's layer accepts only `c` and
  padded base64. (This replaces Task 4's guess that the OSC 52 and `?117`
  differences were `rustty-vt` choices: they are upstream changes.)
- **Prompt marks after DL.** In v1.3.1 `CSI n M` on a prompt row leaves the
  recycled bottom rows marked as prompts, so `find_prompt` finds a phantom
  prompt; fixed upstream in `b31fbc846` (ghostty #13940). Found by the seeded
  fuzzer and pinned by `prompt-delete-lines`.
- **Fixed in this adapter:** the discarded-row count was derived from page
  serials, which also advance when a page is re-laid-out in place (region
  scrolls), when ED 3 erases history and across RIS; the harness saw 24–551
  phantom discarded rows. Replaced by the pin-based count above.
