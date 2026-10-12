import { readFileSync } from "node:fs";
import { GhosttyCore } from "@wterm/ghostty";

// Compiled modules, by path: every engine gets a fresh instance (fresh memory),
// but the harness compiles each wasm file once.
const modules = new Map();

/** Instantiates a wasm file and wraps it in wterm's own GhosttyCore (public constructor). */
export async function loadEngine(path, options = {}) {
  if (!modules.has(path)) modules.set(path, await WebAssembly.compile(readFileSync(path)));
  const instance = await WebAssembly.instantiate(modules.get(path), { env: { log() {} } });
  return new GhosttyCore({ exports: instance.exports, instance }, options);
}
