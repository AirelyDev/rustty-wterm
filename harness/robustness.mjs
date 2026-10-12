#!/usr/bin/env node
// Hostile-input robustness: drives the wasm through its RAW exports and fails
// if any call traps (a Rust panic is `panic=abort`, i.e. an `unreachable`
// trap that kills the instance). Every scenario gets a fresh instance so one
// trap cannot mask the rest; the report names the export and arguments.
//
// Calls are driven from the module's own export list (Task 5 outcome B: no
// `graphics_*` exports), and the REQUIRED set from tools/audit-wasm.mjs must
// be a subset. Freed pointers are excluded (use-after-free is undefined
// behaviour, documented), as are non-null pointers that do not come from
// `alloc_buffer`: the ABI contract is "valid buffer or null".
//
// Usage: node harness/robustness.mjs <wasm>
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = resolve(process.argv[2] ?? "target/wasm32-unknown-unknown/release/rustty_wterm.wasm");
const bytes = readFileSync(wasmPath);
const module = new WebAssembly.Module(bytes);
const exportNames = WebAssembly.Module.exports(module);
const have = new Set(exportNames.map((e) => e.name));

// REQUIRED is the first array literal in the audit script.
const auditSrc = readFileSync(join(here, "../tools/audit-wasm.mjs"), "utf8");
const required = JSON.parse(/const REQUIRED = (\[[\s\S]*?\]);/.exec(auditSrc)[1]);
const missing = required.filter((n) => !have.has(n));
if (missing.length) {
  console.error(`FAIL: required exports missing: ${missing.join(", ")}`);
  process.exit(1);
}

const U32 = 0xffffffff;
const fresh = () => new WebAssembly.Instance(module, {}).exports;

// Indices (after the handle at 0) of params that are buffer pointers.
const PTR_PARAMS = {
  write: [1], get_viewport: [1], get_viewport_v2: [1], get_scrollback_line: [2],
  get_scrollback_line_v2: [2], get_scrollback_grapheme: [3], get_scrollback_hyperlink: [3],
  get_viewport_grapheme: [3], get_viewport_hyperlink: [3], read_response: [1],
  set_theme_colors: [1], free_buffer: [0],
};
// Exports that take no handle.
const NO_HANDLE = new Set(["init", "alloc_buffer", "free_buffer"]);

const fns = exportNames.filter((e) => e.kind === "function").map((e) => e.name);
const failures = [];
let calls = 0;
let scenarios = 0;

function guarded(label, fn) {
  calls++;
  try {
    fn();
  } catch (e) {
    failures.push(`${label}: ${e instanceof WebAssembly.RuntimeError ? "TRAP " : ""}${e.message}`);
    return false;
  }
  return true;
}

/** Run `body(exports, handle)` on a fresh instance; a trap ends only this scenario. */
function scenario(name, body, { populate = false } = {}) {
  scenarios++;
  const x = fresh();
  const h = x.init(80, 24, 100000, 0xd4d4d4, 0x1e1e1e, 0);
  if (!h) { failures.push(`${name}: init returned 0`); return; }
  if (populate) feed(x, h, populateBytes());
  const call = (fnName, ...args) => {
    const label = `[${name}] ${fnName}(${args.map((a) => (a === U32 ? "u32::MAX" : a)).join(", ")})`;
    return guarded(label, () => x[fnName](...args));
  };
  body(x, h, call);
}

function feed(x, h, data) {
  const p = x.alloc_buffer(data.length);
  new Uint8Array(x.memory.buffer, p, data.length).set(data);
  x.write(h, p, data.length);
  x.free_buffer(p, data.length);
}

function populateBytes() {
  const t = new TextEncoder();
  let s = "\x1b]8;;http://x.invalid\x07link\x1b]8;;\x07 é \u{1F468}‍\u{1F469} 漢\r\n";
  for (let i = 0; i < 300; i++) s += `row ${i} \x1b[31mred\x1b[0m\r\n`;
  s += "\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]2;title\x07\x1b[c\x1b[6n";
  return t.encode(s);
}

// 1. Every function export: handle 0, with all-zero and all-u32::MAX args
//    (u16/u8 params truncate, which is fine).
for (const name of fns) {
  const arity = fresh()[name].length;
  for (const fill of [0, U32]) {
    scenario(`null-handle fill=${fill}`, (x, h, call) => {
      // Handle 0 is the documented "no core" value; any other non-live handle is UB.
      const args = Array.from({ length: arity }, (_, i) => (i === 0 && (name === "free_buffer" || !NO_HANDLE.has(name)) ? 0 : fill));
      if (name === "init" || name === "deinit") return;
      call(name, ...args);
    });
  }
}
// init itself with zero / huge sizes (returns 0 or a handle; never traps).
for (const args of [[0, 0, 0, 0, 0, 0], [0, 24, 0, 0, 0, 0], [80, 0, 0, 0, 0, 0],
  [U32, U32, U32, U32, U32, U32], [1, 1, 0, 0, 0, 0], [65535, 65535, 1000, 0, 0, 0]]) {
  scenario(`init ${args}`, (x, h, call) => {
    let r;
    if (guarded(`[init-args] init(${args})`, () => { r = x.init(...args); }) && r) x.deinit(r);
  });
}
scenario("deinit(0)", (x, h, call) => { call("deinit", 0); });

// 2. Valid handle, null buffers, u32::MAX offsets/lengths, one param at a
//    time and all at once; populated (scrollback, graphemes, links) and empty.
for (const populate of [false, true]) {
  for (const name of fns) {
    if (NO_HANDLE.has(name) || name === "deinit" || name === "resize") continue;
    const arity = fresh()[name].length;
    const ptrs = new Set(PTR_PARAMS[name] ?? []);
    const variants = [];
    variants.push(Array.from({ length: arity - 1 }, () => 0));
    variants.push(Array.from({ length: arity - 1 }, (_, i) => (ptrs.has(i + 1) ? 0 : U32)));
    for (let i = 1; i < arity; i++) {
      if (ptrs.has(i)) continue;
      variants.push(Array.from({ length: arity - 1 }, (_, j) => (j + 1 === i ? U32 : 0)));
    }
    for (const rest of variants) {
      scenario(`${populate ? "populated" : "empty"} ${name}`, (x, h, call) => {
        call(name, h, ...rest);
        // The instance must still answer afterwards (no corrupted state).
        call("get_cols", h);
        call("update", h);
      }, { populate });
    }
  }
}

// 3. Allocation extremes (u32::MAX returns 0 rather than trapping).
scenario("alloc_buffer extremes", (x, h, call) => {
  for (const n of [0, 1, U32, 0x7fffffff, 0x80000000]) {
    let p;
    if (call("alloc_buffer", n)) { p = x.alloc_buffer(n); if (p) x.free_buffer(p, n); }
  }
  call("free_buffer", 0, U32);
});

// 4. Resize extremes, then a read of everything.
scenario("resize extremes", (x, h, call) => {
  for (const [c, r] of [[0, 0], [0, 24], [80, 0], [65535, 65535], [1, 1], [2, 1], [1, 2], [80, 24]]) {
    call("resize", h, c, r);
    call("update", h);
    call("get_cursor_row", h);
    call("get_cursor_col", h);
    call("find_prompt", h, 0, 1);
    call("find_prompt", h, U32, -1);
  }
}, { populate: true });

scenario("resize to the largest supported grid, alt screen, reads", (x, h, call) => {
  call("resize", h, 8000, 8000);
  call("update", h);
  feed(x, h, new TextEncoder().encode("\x1b[?1049hhi\x1b[?1049l\x1b[2J"));
  call("update", h);
  call("resize", h, 20000, 20000);
  call("update", h);
  call("resize", h, 80, 24);
  call("update", h);
});

scenario("shrink then read viewport/scrollback", (x, h, call) => {
  call("resize", h, 5, 2);
  call("update", h);
  const cols = x.get_cols(h), rows = x.get_rows(h);
  for (const [fn, cb] of [["get_viewport", 16], ["get_viewport_v2", 20]]) {
    const n = cols * rows * cb;
    const p = x.alloc_buffer(n);
    call(fn, h, p);
    x.free_buffer(p, n);
  }
  const n = Math.max(1, x.get_scrollback_count(h));
  for (const off of [0, n - 1, n, n + 1, U32]) {
    const p = x.alloc_buffer(cols * 20);
    call("get_scrollback_line_v2", h, off, p, cols);
    call("get_scrollback_line", h, off, p, cols);
    x.free_buffer(p, cols * 20);
  }
}, { populate: true });

// 5. Hostile write payloads.
const big = (n, f) => { const a = new Uint8Array(n); for (let i = 0; i < n; i++) a[i] = f(i); return a; };
const te = new TextEncoder();
const payloads = {
  "64KB OSC 52": (() => { const b = new Uint8Array(65536 + 12); b.set(te.encode("\x1b]52;c;")); b.fill(0x41, 7, b.length - 1); b[b.length - 1] = 7; return b; })(),
  "64KB OSC 0 title": (() => { const b = new Uint8Array(65536 + 6); b.set(te.encode("\x1b]0;")); b.fill(0x58, 4, b.length - 1); b[b.length - 1] = 7; return b; })(),
  "unterminated OSC 8": te.encode("\x1b]8;;" + "u".repeat(70000)),
  "4MB APC": (() => { const b = new Uint8Array((4 << 20) + 8); b.set(te.encode("\x1b_G")); b.fill(0x41, 3, b.length - 2); b.set(te.encode("\x1b\\"), b.length - 2); return b; })(),
  "kitty graphics chunks": te.encode("\x1b_Ga=T,f=24,s=10000,v=10000,m=1;AAAA\x1b\\\x1b_Gm=0;AAAA\x1b\\\x1b_Ga=d\x1b\\"),
  "huge CSI params": te.encode("\x1b[" + "9;".repeat(5000) + "H\x1b[99999999999999999999A\x1b[4294967295@\x1b[65536;65536H\x1b[999999999L\x1b[999999999P"),
  "SGR overflow": te.encode("\x1b[38;2;999;999;999m\x1b[38;5;999m\x1b[58;2;;;m\x1b[4:99m" + "\x1b[1;".repeat(500) + "m"),
  "invalid UTF-8": new Uint8Array([0xff, 0xfe, 0xc0, 0x80, 0xe0, 0x80, 0xf8, 0xed, 0xa0, 0x80, 0xf4, 0x90, 0x80, 0x80, 0xc3]),
  "combining flood": te.encode("a" + "́".repeat(20000) + "‍".repeat(2000) + "\r\n"),
  "scroll region abuse": te.encode("\x1b[0;0r\x1b[999;1r\x1b[5;2r\x1b[?69h\x1b[9;3s\x1b[100000S\x1b[100000T\x1b[100000M\x1b[100000L\x1b[100000X"),
  "alt-screen toggling": te.encode("\x1b[?1049h".repeat(500) + "\x1b[?1049l\x1b[?47h\x1b[?1047l\x1b[?1048h"),
  "full reset loop": te.encode("\x1bc".repeat(2000) + "\x1b#8\x1bc"),
  "prompt marks": te.encode("\x1b]133;A\x07\x1b]133;D;99999999999\x07\x1b]133;D;-1\x07\x1b]133;\x07\x1b]133;Z\x07".repeat(200)),
  "tab stops": te.encode("\x1b[99999I\x1b[99999Z\x1b[3g\x1bH".repeat(50)),
  "ESC at end": te.encode("text\x1b"),
};
for (const [name, data] of Object.entries(payloads)) {
  scenario(`write ${name}`, (x, h, call) => {
    const p = x.alloc_buffer(data.length);
    new Uint8Array(x.memory.buffer, p, data.length).set(data);
    if (call("write", h, p, data.length)) {
      call("update", h);
      // Read everything back through the raw exports.
      const cols = x.get_cols(h), rows = x.get_rows(h);
      const vp = x.alloc_buffer(cols * rows * 20);
      call("get_viewport_v2", h, vp);
      x.free_buffer(vp, cols * rows * 20);
      call("get_scrollback_count", h);
      call("get_clipboard_write_len", h);
      call("get_title_len", h);
      const rb = x.alloc_buffer(256);
      for (let i = 0; i < 300; i++) if (!x.read_response(h, rb, 256)) break;
      x.free_buffer(rb, 256);
    }
    x.free_buffer(p, data.length);
  });
}
// Garbage bytes (xorshift), 64 x 64 KB, interleaved with update + resize.
scenario("random garbage 4MB", (x, h, call) => {
  let s = 0x2545f491n;
  const MASK = (1n << 64n) - 1n;
  const buf = new Uint8Array(1 << 16);
  const p = x.alloc_buffer(buf.length);
  for (let round = 0; round < 64; round++) {
    for (let i = 0; i < buf.length; i++) {
      s ^= (s << 13n) & MASK; s ^= s >> 7n; s ^= (s << 17n) & MASK; buf[i] = Number(s & 0xffn);
    }
    new Uint8Array(x.memory.buffer, p, buf.length).set(buf);
    if (!call("write", h, p, buf.length)) return;
    call("update", h);
    if (round % 8 === 0) call("resize", h, 1 + (round * 7) % 120, 1 + (round * 3) % 40);
  }
  x.free_buffer(p, buf.length);
});

// 6. Tracked positions: exhaust the table, release bogus ids, resolve after resize.
scenario("tracked positions", (x, h, call) => {
  for (let i = 0; i < 100; i++) call("track_position", h, i % 30, i % 100);
  for (const id of [0, 1, 63, 64, 65, U32]) { call("release_position", h, id); call("release_position", h, id); call("resolve_position", h, id); }
  call("track_position", h, U32, U32);
  call("resize", h, 3, 3);
  for (let id = 0; id < 70; id++) call("resolve_position", h, id);
}, { populate: true });

if (failures.length) {
  console.error(`FAIL: ${failures.length} trap(s) in ${scenarios} scenarios / ${calls} calls`);
  for (const f of failures) console.error(`  ${f}`);
  process.exit(1);
}
console.log(`robustness ok: ${fns.length} function exports (${required.length} required present), ${scenarios} scenarios, ${calls} guarded calls, no traps`);
