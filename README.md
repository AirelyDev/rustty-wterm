# rustty-wterm

An implementation of the `ghostty-vt.wasm` ABI that [wterm](https://github.com/vercel-labs/wterm)
(`@wterm/ghostty`) loads, written in Rust on top of the Rustty VT engine
(`rustty-vt`). It exports the same functions with the same byte layouts and
quirks as wterm's Zig layer (`packages/@wterm/ghostty/zig/src/wasm_api.zig`),
so wterm's unmodified JavaScript (`GhosttyCore`) can run on either engine by
changing only the wasm URL. The release build is `wasm32-unknown-unknown` with
zero imports (the Zig build imports `env.log`; this one imports nothing).

This repository is separate from Rustty and ports nothing from Ghostty; it is
an adapter between two other projects' interfaces.

## ABI

Every export of wterm's `ghostty-vt.wasm` and the lines of `wasm_api.zig`
(wterm `488e9a7344e812e35a951f803618d7767cc4be5b`) whose behaviour it mirrors.
The Rust build exports 55 functions plus `memory`; the Zig build exports the
same plus the 11 `graphics_*` functions. `tools/audit-wasm.mjs` fails the
build if the wasm has any import, lacks any required export, or exports a
`graphics*` name.

| Export | `wasm_api.zig` lines | Status |
|---|---|---|
| `set_theme_colors` | 690-700 | exported |
| `init` | 702-764 | exported |
| `deinit` | 766-773 | exported |
| `resize` | 775-779 | exported |
| `write` | 783-787 | exported |
| `get_title_len` | 791-796 | exported |
| `get_title_ptr` | 798-800 | exported |
| `get_working_directory_len` | 803-808 | exported |
| `get_working_directory_ptr` | 810-812 | exported |
| `get_bell_count` | 814-819 | exported |
| `get_clipboard_write_len` | 821-826 | exported |
| `get_clipboard_write_ptr` | 828-830 | exported |
| `get_shell_phase` | 833-838 | exported |
| `get_shell_exit_code` | 840-845 | exported |
| `update` | 849-852 | exported |
| `get_color_override` | 855-864 | exported |
| `get_viewport` | 963-965 | exported |
| `get_viewport_v2` | 967-969 | exported |
| `get_viewport_grapheme` | 1039-1060 | exported |
| `get_viewport_hyperlink` | 1100-1113 | exported |
| `is_dirty` | 1117-1124 | exported |
| `is_dirty_row` | 1126-1131 | exported |
| `clear_dirty` | 1133-1138 | exported |
| `get_cursor_row` | 1142-1145 | exported |
| `get_cursor_col` | 1147-1150 | exported |
| `get_cursor_visible` | 1152-1155 | exported |
| `get_cursor_shape` | 1157-1163 | exported |
| `get_cursor_blinking` | 1165-1167 | exported |
| `cursor_keys_app` | 1171-1174 | exported |
| `bracketed_paste` | 1176-1179 | exported |
| `using_alt_screen` | 1181-1184 | exported |
| `mouse_tracking` | 1186-1194 | exported |
| `mouse_sgr` | 1196-1199 | exported |
| `mouse_encoding` | 1201-1204 | exported |
| `focus_events` | 1206-1209 | exported |
| `synchronized_output` | 1211-1214 | exported |
| `synchronized_output_generation` | 1216-1219 | exported |
| `kitty_keyboard_flags` | 1221-1224 | exported |
| `graphics_generation` | 1228-1230 | intentionally absent (Kitty graphics, see below) |
| `graphics_image_count` | 1232-1234 | intentionally absent (Kitty graphics, see below) |
| `graphics_placement_count` | 1236-1238 | intentionally absent (Kitty graphics, see below) |
| `graphics_bytes_used` | 1240-1242 | intentionally absent (Kitty graphics, see below) |
| `graphics_capacity` | 1244-1246 | intentionally absent (Kitty graphics, see below) |
| `graphics_rejected` | 1248-1250 | intentionally absent (Kitty graphics, see below) |
| `graphics_evicted` | 1252-1254 | intentionally absent (Kitty graphics, see below) |
| `graphics_images` | 1256-1259 | intentionally absent (Kitty graphics, see below) |
| `graphics_placements` | 1261-1264 | intentionally absent (Kitty graphics, see below) |
| `graphics_image_size` | 1281-1289 | intentionally absent (Kitty graphics, see below) |
| `graphics_image_copy` | 1291-1313 | intentionally absent (Kitty graphics, see below) |
| `get_cols` | 1317-1320 | exported |
| `get_rows` | 1322-1325 | exported |
| `find_prompt` | 1331-1361 | exported |
| `track_position` | 1369-1384 | exported |
| `resolve_position` | 1386-1396 | exported |
| `release_position` | 1398-1406 | exported |
| `get_row_wraps` | 1416-1422 | exported |
| `get_scrollback_row_wraps` | 1424-1430 | exported |
| `get_scrollback_count` | 1432-1442 | exported |
| `get_scrollback_discarded_count` | 1444-1447 | exported |
| `get_scrollback_line` | 1459-1461 | exported |
| `get_scrollback_line_v2` | 1463-1465 | exported |
| `get_scrollback_grapheme` | 1493-1512 | exported |
| `get_scrollback_hyperlink` | 1514-1526 | exported |
| `read_response` | 1530-1533 | exported |
| `alloc_buffer` | 1537-1540 | exported |
| `free_buffer` | 1542-1545 | exported |
| `memory` | n/a (linear memory) | exported |

wterm's `GhosttyExports` marks every `graphics_*` export optional; without
them `GhosttyCore.getGraphicsState()` reports no images (see "Kitty graphics").

## Pins

| What | Pin | How it is fixed |
|---|---|---|
| `rustty-vt` | Rustty commit `3e43a1d18119e29ce99092ee33b1c7b134ad7834` | path dependency `../rustty/crates/rustty-vt` (read-only); `git -C ../rustty rev-parse HEAD` and a clean `git status --short crates/rustty-vt` there when recorded |
| wterm reference | `488e9a7344e812e35a951f803618d7767cc4be5b` | the `wasm_api.zig` lines above; test expectations |
| `@wterm/*` | 0.5.4 | exact versions in `harness/package.json` (the website pins the same) |
| Ghostty oracle for Rustty | `246f70287` | Rustty's current reference commit (`.port/` notes in the Rustty repository) |
| Ghostty inside wterm's Zig build | v1.3.1 | per the harness findings below |
| Rust toolchain | 1.93.1 | `rust-toolchain.toml` |

The path dependency is not a lock: `Cargo.lock` does not record a commit for
it, so the `rustty-vt` row is only as meaningful as the state of the sibling
checkout. Rebuild against a different Rustty checkout and the harness results
below may change.

## Build

```sh
tools/build-wasm.sh          # release wasm + audit (zero imports, required exports)
tools/build-wasm.sh --diff   # ... then the differential harness
```

The output is `target/wasm32-unknown-unknown/release/rustty_wterm.wasm`.

## Test

```sh
tools/check.sh               # everything: fmt, clippy, tests, wasm build, harness
cargo test                   # host tests only
```

`tools/check.sh` runs `cargo fmt --check`, `cargo clippy --all-targets -- -D
warnings`, `cargo test`, regenerates the harness corpus (failing if the
committed files are stale), `tools/build-wasm.sh`, `node harness/diff.mjs` and
`node harness/robustness.mjs`; it exits 0 only when all pass. Last run:
exit 0; 65 host tests passed; audit `ok: 58 exports, 0 imports` (the 55
functions, `memory`, and the linker globals `__data_end` and `__heap_base`);
`comparisons: 280`, `corpus entries: 64, identical: 50, known: 14 (skipped: 1)`;
`robustness ok: 55 function exports (56 required present), 394 scenarios,
1372 guarded calls, no traps`.

## Code provenance

The cell encoder (`src/cell.rs`), the sinks (`src/sinks.rs`), the tracked
position table and prompt search (`src/positions.rs`) and the scrollback /
grapheme / hyperlink readers (`src/scrollback.rs`) are close ports of the
corresponding `wasm_api.zig` functions and structs (line ranges in the module
headers). `src/core.rs`, `src/effects.rs` and `src/exports.rs` follow the Zig
layer's structure but are written against `rustty-vt`'s API, which differs
from the libghostty internals the Zig code uses.

## Differential harness

`harness/` feeds the same bytes to this build and to wterm's own
`ghostty-vt.wasm` (`@wterm/ghostty@0.5.4`, ghostty v1.3.1), both through
wterm's unmodified `GhosttyCore`, and compares every observable: cells,
scrollback, responses, cursor, title, cwd, bell, clipboard, shell state,
colour overrides, modes, wrap metadata, discarded rows, prompts and tracked
positions. Every difference is either fixed or listed with evidence in
`harness/known-diffs.json`. See `harness/README.md`.

Real-program recordings (`harness/corpus/real-*.bin`: `ls`, `git log`, `vim`,
`top`, `less`) are part of the corpus and were identical in both engines with
no new allowance. `node harness/robustness.mjs <wasm>` and `tests/hostile.rs`
prove that no export traps on hostile input (null handle and buffers,
`u32::MAX` offsets, extreme resizes, 64 KB OSC 52, 4 MB APC, garbage).

Grids over 64 M cells (8000 x 8000) are refused by `init` and `resize`: on
wasm32 a larger resize fails inside the engine on allocation and leaves the
screen inconsistent, so the next `update` aborted (found by the robustness
script). `[profile.dev.package.rustty-vt]` builds the engine without debug
assertions in tests: its per-page integrity check makes a 65535 x 65535
resize take minutes.

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
  from half a page of input bytes, which only REP (`CSI n b`), SU
  (`CSI n S`) with a large count, or a run of ED 2 scroll-clears on very wide
  or very short pages can produce.

## Findings

What the differential harness found. `harness/known-diffs.json` lists 19
entries with evidence for each: 18 are owner `skew` (upstream Ghostty
changes between wterm's v1.3.1 and Rustty's oracle `246f70287`, which
`rustty-vt` reproduces) and 1 is owner `rustty-vt` (`tracked-alt-screen`, the
screen-switch gap above). Entries are grouped by cause below.
No difference is a `rustty-vt` porting bug: apart from the gap above, every
engine difference traces to an upstream ghostty commit between wterm's v1.3.1
and Rustty's oracle `246f70287`, and `rustty-vt` reproduces the newer
behaviour.

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
  prompt; fixed upstream in `b31fbc846` (ghostty #13940). The same commit
  resets rows recycled by scroll-region SU, RI and region-bottom linefeeds, so
  v1.3.1 also keeps stale prompt marks and wrap flags there. Found by the
  seeded fuzzer and pinned by `prompt-delete-lines`, `prompt-region-su`,
  `prompt-region-ri` and `wrap-flag-region-scroll`.
- **Fixed in this adapter:** the discarded-row count was derived from page
  serials, which also advance when a page is re-laid-out in place (region
  scrolls), when ED 3 erases history and across RIS; the harness saw 24–551
  phantom discarded rows. Replaced by the pin-based count above.

## Licence and attribution

This crate is MIT (`Cargo.toml` `license`; text in `LICENSE`, the same text
and copyright holder lines as Rustty's). It reproduces the ABI, byte layouts
and observable behaviour of wterm's `wasm_api.zig`, and reuses wterm's test
expectations and unmodified JavaScript in the harness. wterm is by Vercel
Labs and is Apache-2.0; see `NOTICE`. Ghostty (MIT) is the behavioural oracle
through Rustty. The website that serves this wasm also ships wterm's
Apache-2.0 `LICENSE` next to the binaries.
