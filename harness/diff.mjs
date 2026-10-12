#!/usr/bin/env node
// Differential harness: feeds every corpus entry to the Rust build and to the
// reference ghostty-vt.wasm, both wrapped in wterm's own unmodified
// GhosttyCore, and compares the observable state.
//
// Usage: node harness/diff.mjs [--rust <wasm>] [--zig <wasm>] [--only <name>] [--verbose]
// Exit 0 iff every entry is identical modulo known-diffs.json and every
// known diff was used.
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { GhosttyCore } from "@wterm/ghostty";
import { loadEngine } from "./lib/load-core.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");

const args = process.argv.slice(2);
function flag(name, fallback) {
  const i = args.indexOf(name);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : fallback;
}
const rustPath = resolve(flag("--rust", join(root, "target/wasm32-unknown-unknown/release/rustty_wterm.wasm")));
const zigPath = resolve(flag("--zig", join(here, "node_modules/@wterm/ghostty/wasm/ghostty-vt.wasm")));
const only = flag("--only", null);
const verbose = args.includes("--verbose");
const MAX_PATHS = 20; // per top-level snapshot field

const COLS = 80;
const ROWS = 24;
const BASE_OPTIONS = { scrollbackLimit: 1000, foregroundColor: "#d4d4d4", backgroundColor: "#1e1e1e" };

const manifest = JSON.parse(readFileSync(join(here, "corpus/manifest.json"), "utf8"));
const knownDiffs = JSON.parse(readFileSync(join(here, "known-diffs.json"), "utf8"));
for (const k of knownDiffs) {
  for (const key of ["entry", "path", "reason", "owner"]) {
    if (typeof k[key] !== "string" || !k[key]) throw new Error(`known-diffs.json: entry missing ${key}: ${JSON.stringify(k)}`);
  }
  if (!["rustty-vt", "wterm", "skew"].includes(k.owner)) throw new Error(`known-diffs.json: bad owner ${k.owner}`);
}
const knownUsed = new Set();

// -- Snapshot -------------------------------------------------------------

function snapshot(core, tracked) {
  core.writeString(""); // no-op, ensures update path
  const rows = core.getRows(), cols = core.getCols();
  const grid = [];
  for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) grid.push(core.getCell(r, c));
  const sb = [];
  const sbWraps = [];
  for (let o = 0; o < core.getScrollbackCount(); o++) {
    const n = core.getScrollbackLineLen(o);
    for (let c = 0; c < n; c++) sb.push(core.getScrollbackCell(o, c));
    sbWraps.push(core.getScrollbackRowMetadata(o));
  }
  const responses = []; for (let r; (r = core.getResponse()) !== null; ) responses.push(r);
  const scrollbackCount = core.getScrollbackCount();
  const prompts = [];
  for (let r = 0; r < scrollbackCount + rows; r++) prompts.push([core.findPrompt(r, -1), core.findPrompt(r, 1)]);
  return {
    grid, sb, responses,
    cursor: core.getCursor(), title: core.getTitle(), cwd: core.getWorkingDirectory?.(),
    bells: core.getBellCount?.(), clipboard: core.getClipboardWrite?.(),
    shell: core.getShellIntegrationState?.(), overrides: core.getColorOverrides?.(),
    modes: [core.cursorKeysApp(), core.bracketedPaste(), core.usingAltScreen(),
            core.mouseTracking?.(), core.mouseEncoding?.(), core.focusEvents?.(),
            core.synchronizedOutput?.(), core.kittyKeyboardFlags?.()],
    wraps: Array.from({ length: rows }, (_, r) => core.getRowMetadata?.(r)),
    scrollbackCount,
    scrollbackDiscarded: core.getScrollbackDiscardedCount(),
    sbWraps,
    prompts,
    tracked: tracked.map((t) => t.resolve()),
  };
}

// -- Structural diff ------------------------------------------------------

function fmt(v, path) {
  if (typeof v === "number" && /Rgb$/.test(path)) return `0x${v.toString(16).padStart(6, "0")}`;
  return JSON.stringify(v);
}
function diffPaths(a, b, path = "", out = []) {
  if (out.length >= MAX_PATHS) return out;
  if (Object.is(a, b)) return out;
  const ao = a !== null && typeof a === "object";
  const bo = b !== null && typeof b === "object";
  if (!ao || !bo || Array.isArray(a) !== Array.isArray(b)) {
    out.push({ path, a, b });
    return out;
  }
  if (Array.isArray(a)) {
    for (let i = 0; i < Math.max(a.length, b.length); i++) diffPaths(a[i], b[i], `${path}[${i}]`, out);
    return out;
  }
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const k of [...keys].sort()) {
    if (a[k] === undefined && b[k] === undefined) continue;
    diffPaths(a[k], b[k], path ? `${path}.${k}` : k, out);
  }
  return out;
}

/** A known path matches itself and everything below it (`sb` covers `sb[12].char`). */
function pathMatches(known, path) {
  return path === known || path.startsWith(`${known}[`) || path.startsWith(`${known}.`);
}

// -- Running an entry -----------------------------------------------------

const files = new Map();
function corpusBytes(name) {
  if (!files.has(name)) files.set(name, new Uint8Array(readFileSync(join(here, "corpus", name))));
  return files.get(name);
}

function chunk(bytes, mode) {
  if (mode === "whole") return [bytes];
  const size = mode === "bytes" ? 1 : 7;
  const out = [];
  for (let i = 0; i < bytes.length; i += size) out.push(bytes.subarray(i, i + size));
  return out;
}

async function run(path, entry, writeChunks) {
  const core = await loadEngine(path, { ...BASE_OPTIONS, ...(entry.options ?? {}) });
  core.init(COLS, ROWS);
  const tracked = [];
  let w = 0;
  for (const step of entry.steps) {
    if (step.write) for (const c of writeChunks(corpusBytes(step.write), w++)) core.writeRaw(c);
    else if (step.resize) core.resize(...step.resize);
    else if (step.track) tracked.push(core.trackPosition({ row: step.track[0], col: step.track[1] }));
    else throw new Error(`${entry.name}: unknown step ${JSON.stringify(step)}`);
  }
  const snap = snapshot(core, tracked.map((t) => t ?? { resolve: () => "untracked" }));
  core.dispose();
  return snap;
}

function chunkings(entry) {
  if (entry.chunkings !== "splits") {
    return ["whole", "bytes", "7-byte"].map((mode) => ({
      label: mode,
      writeChunks: (bytes) => chunk(bytes, mode === "7-byte" ? "seven" : mode),
    }));
  }
  const len = entry.steps.reduce((n, s) => (s.write ? Math.max(n, corpusBytes(s.write).length) : n), 0);
  const out = [];
  for (let k = 0; k <= len; k++) {
    out.push({
      label: `split@${k}`,
      writeChunks: (bytes) => [bytes.subarray(0, k), bytes.subarray(k)].filter((c) => c.length > 0),
    });
  }
  return out;
}

// -- Adapter checks: GhosttyCore must work with the Rust build's export set --

async function adapterChecks() {
  const problems = [];
  const core = await loadEngine(rustPath, BASE_OPTIONS);
  core.init(COLS, ROWS);
  core.writeString("\x1b_Ga=T,f=24,s=2,v=1,i=7;/wAAAP8A\x1b\\ok");
  try {
    const state = core.getGraphicsState();
    if (state !== null) problems.push(`getGraphicsState() = ${JSON.stringify(state)}, expected null`);
    const image = core.getGraphicsImage(7, 1);
    if (image !== null) problems.push(`getGraphicsImage() = ${JSON.stringify(image)}, expected null`);
    const res = core.getResourceState();
    if (JSON.stringify(res) !== "{}") problems.push(`getResourceState() = ${JSON.stringify(res)}, expected {}`);
    if (core.getCell(0, 0).char !== "o".codePointAt(0)) problems.push("text after an image command was lost");
  } catch (e) {
    problems.push(`graphics accessors threw: ${e}`);
  }
  core.dispose();

  // GhosttyCore.load(), the path the website uses, over real HTTP.
  const server = createServer((req, res) => {
    res.writeHead(200, { "Content-Type": req.url.endsWith("octet") ? "application/octet-stream" : "application/wasm" });
    res.end(readFileSync(rustPath));
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const { port } = server.address();
  try {
    for (const suffix of ["wasm", "octet"]) {
      const loaded = await GhosttyCore.load({ ...BASE_OPTIONS, wasmPath: `http://127.0.0.1:${port}/rustty.${suffix}` });
      loaded.init(COLS, ROWS);
      loaded.writeString("load\x1b[6n");
      if (loaded.getCell(0, 0).char !== "l".codePointAt(0)) problems.push(`GhosttyCore.load (${suffix}): text missing`);
      if (loaded.getResponse() !== "\x1b[1;5R") problems.push(`GhosttyCore.load (${suffix}): CPR missing`);
      loaded.dispose();
    }
  } catch (e) {
    problems.push(`GhosttyCore.load threw: ${e}`);
  } finally {
    server.close();
  }
  return problems;
}

// -- Main -----------------------------------------------------------------

const adapterProblems = await adapterChecks();
for (const p of adapterProblems) console.log(`ADAPTER FAIL ${p}`);
if (adapterProblems.length === 0) {
  console.log("adapter: GhosttyCore runs the Rust build (constructor and load(), no graphics_* exports)");
}

let total = 0, identical = 0, known = 0, failed = 0, skipped = 0, comparisons = 0;
for (const entry of manifest.entries) {
  if (only && entry.name !== only) continue;
  if (entry.skip) {
    skipped++;
    console.log(`skip ${entry.name}: ${entry.skip}`);
    continue;
  }
  total++;
  let entryKnown = false, entryFailed = false;
  for (const { label, writeChunks } of chunkings(entry)) {
    const [a, b] = [await run(rustPath, entry, writeChunks), await run(zigPath, entry, writeChunks)];
    comparisons++;
    // Per top-level field, in snapshot order, so one noisy field cannot
    // hide another.
    const diffs = Object.keys(a).flatMap((key) => diffPaths(a[key], b[key], key));
    const unknown = [];
    for (const d of diffs) {
      const k = knownDiffs.findIndex((x) => x.entry === entry.name && pathMatches(x.path, d.path));
      if (k >= 0) {
        knownUsed.add(k);
        entryKnown = true;
        if (verbose) console.log(`  known ${entry.name} [${label}] ${d.path}: ${fmt(d.a, d.path)} != ${fmt(d.b, d.path)}`);
      } else unknown.push(d);
    }
    if (unknown.length > 0) {
      entryFailed = true;
      console.log(`FAIL ${entry.name} [${label}]`);
      for (const d of (verbose ? unknown : unknown.slice(0, 3))) console.log(`  ${d.path}: ${fmt(d.a, d.path)} != ${fmt(d.b, d.path)}   (rust != zig)`);
      if (!verbose) break; // first failing chunking is enough
    }
  }
  if (entryFailed) failed++;
  else if (entryKnown) known++;
  else identical++;
}

let staleKnown = 0;
if (!only) {
  knownDiffs.forEach((k, i) => {
    if (!knownUsed.has(i)) {
      staleKnown++;
      console.log(`UNUSED known diff ${k.entry} ${k.path} (owner ${k.owner}) — remove it`);
    }
  });
}

console.log(`comparisons: ${comparisons} (entry x chunking, both engines through GhosttyCore)`);
console.log(`corpus entries: ${total}, identical: ${identical}, known: ${known}` +
  (skipped ? ` (skipped: ${skipped})` : "") + (failed ? `, FAILED: ${failed}` : ""));
process.exit(failed || staleKnown || adapterProblems.length ? 1 : 0);
