# Differential harness

Feeds identical bytes to two engines and compares everything wterm can observe:

- **Rust engine**: `target/wasm32-unknown-unknown/release/rustty_wterm.wasm` (`tools/build-wasm.sh`)
- **Zig engine**: `node_modules/@wterm/ghostty/wasm/ghostty-vt.wasm` from the exact-pinned
  `@wterm/ghostty@0.5.4` (ghostty v1.3.1 plus wterm's wasm patches)

Both are driven through wterm's own, unmodified `GhosttyCore`
(`lib/load-core.mjs` instantiates the wasm and calls the public constructor),
so a pass also proves that the Rust build satisfies the JS adapter, not just
the raw ABI.

```sh
cd harness && npm ci && cd ..
tools/build-wasm.sh --diff          # or:
node harness/diff.mjs [--rust <wasm>] [--zig <wasm>] [--only <entry>] [--verbose]
```

`--only <entry>` runs one entry and skips the unused-known-diff and coverage
checks (they need the whole corpus).

Exit 0 iff every corpus entry is identical in both engines modulo
`known-diffs.json`, every known diff was used, and the adapter checks pass. The
last line reads `corpus entries: N, identical: M, known: K`.

## What is compared

For each entry, each engine starts at 80x24, scrollback limit 1000 bytes
(unless the entry overrides it), fg `#d4d4d4` / bg `#1e1e1e`. After the
entry's steps (writes, resizes, tracked positions) the snapshot holds: every
viewport cell (`getCell`, including graphemes, hyperlinks, underline style and
colour), every scrollback cell and its wrap metadata, every queued response,
cursor (position, visibility, shape, blink), title, working directory, bell
count, clipboard write, shell-integration state, colour overrides, the eight
mode getters, viewport wrap metadata, scrollback count, discarded-row count,
`findPrompt(row, ±1)` for every row, and resolved tracked positions.

Each entry runs in three chunkings: whole, byte by byte, and 7-byte chunks.
`chunked-split` instead runs every two-part split of its bytes.

## Adapter checks

Before the corpus, the harness checks that `GhosttyCore` tolerates the Rust
build's missing `graphics_*` exports (`getGraphicsState()` and
`getGraphicsImage()` return null, `getResourceState()` returns `{}`, text after
a kitty image command survives) and that `GhosttyCore.load({ wasmPath })`, the
path the website uses, loads the Rust build over HTTP served both as
`application/wasm` and as `application/octet-stream`.

## Corpus

`corpus/gen.mjs` writes `corpus/<name>.bin` and `corpus/manifest.json`. The
generated files are committed; `tools/check.sh` regenerates them and fails if
they changed. `random-fuzz-<seed>` mixes 50 fragments per seed with a seeded
PRNG over 20 seeds (1 000 mixes); the fuzz pool uses a reduced query fragment
because the full DECRQM sweep's known replies are pinned by `queries`.
The run also fails if no snapshot anywhere in the corpus has non-empty colour
overrides or non-zero kitty keyboard flags (`colors-override-left-set`,
`kitty-keyboard-flags-left-set`, `kitty-keyboard-alt-screen` and the fuzz pool
leave them set), so those comparisons cannot silently become `{} === {}`.
`kitty-image-rgb` is skipped: the Rust build has no kitty graphics (Task 5
outcome B).

## Real-program recordings

`corpus/real-*.bin` are recorded once by `corpus/record.sh` (macOS: `ls`, `git
log`, `vim`, `top`, and `less`/`bat` when installed) from `corpus/fixture/` and
a throwaway fixed git repo, under an 80x24 pty with pinned `TERM`, `LC_ALL`, `TZ`
and git identity/dates; user, home, host and temp paths are scrubbed. They are
committed and never touched by `gen.mjs`, which only registers them as entries
and refuses any over 64 KB. `record.sh` is re-runnable but is not part of
`tools/check.sh`.

## Robustness

`node harness/robustness.mjs <wasm>` drives the wasm's own export list with a
null handle, null buffers, `u32::MAX` offsets/lengths, extreme resizes and
hostile payloads (64 KB OSC 52, 4 MB APC, CSI/SGR overflow, garbage), one fresh
instance per scenario, and fails naming the export and arguments of any trap.
Freed or arbitrary non-null pointers are excluded (undefined behaviour).

## Known diffs

`known-diffs.json` is a list of
`{ "entry", "path", "owner": "rustty-vt" | "wterm" | "skew", "reason" }`.
A difference is allowed only when both `entry` and `path` match; a `path`
covers itself and everything below it (`sb` covers `sb[12].char`), and the
same allowance holds in every chunking. An entry that no run uses fails the
harness, so stale allowances cannot linger.

Triage of a new difference:

1. **Our bug** (adapter code wrong): fix it in `src/` with a regression test
   next to the originating task's tests.
2. **Version skew** (wterm's ghostty v1.3.1 vs Rustty's oracle commit
   `246f70287`): add with `"owner": "skew"` and the commit evidence.
3. **Rustty engine difference**: do not patch `rustty-vt`; add with
   `"owner": "rustty-vt"` and a note under "Findings" in the top-level README.

Never add a known diff without a reason that names evidence.
