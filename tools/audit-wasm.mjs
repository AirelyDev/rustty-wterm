import { readFileSync } from "node:fs";

/** REQUIRED lists every export the wasm must carry; later tasks append to it. */
const REQUIRED = ["init", "deinit", "resize", "write", "get_cols", "get_rows", "alloc_buffer", "free_buffer", "memory",
  "update", "get_viewport", "get_viewport_v2", "is_dirty", "is_dirty_row", "clear_dirty",
  "get_cursor_row", "get_cursor_col", "get_cursor_visible", "get_cursor_shape", "get_cursor_blinking",
  "cursor_keys_app", "bracketed_paste", "using_alt_screen", "mouse_tracking", "mouse_sgr", "mouse_encoding",
  "focus_events", "synchronized_output", "kitty_keyboard_flags"];

const module = new WebAssembly.Module(readFileSync(process.argv[2]));
const imports = WebAssembly.Module.imports(module);
const exports = new Set(WebAssembly.Module.exports(module).map((e) => e.name));
const missing = REQUIRED.filter((n) => !exports.has(n));
if (imports.length || missing.length) {
  console.error("imports:", imports.map((i) => `${i.module}.${i.name}`), "missing:", missing);
  process.exit(1);
}
console.log(`ok: ${exports.size} exports, 0 imports`);
