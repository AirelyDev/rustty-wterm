import { readFileSync } from "node:fs";

/** REQUIRED lists every export the wasm must carry; later tasks append to it. */
const REQUIRED = ["init", "deinit", "resize", "write", "get_cols", "get_rows", "alloc_buffer", "free_buffer", "memory",
  "update", "get_viewport", "get_viewport_v2", "is_dirty", "is_dirty_row", "clear_dirty",
  "get_cursor_row", "get_cursor_col", "get_cursor_visible", "get_cursor_shape", "get_cursor_blinking",
  "cursor_keys_app", "bracketed_paste", "using_alt_screen", "mouse_tracking", "mouse_sgr", "mouse_encoding",
  "focus_events", "synchronized_output", "kitty_keyboard_flags",
  "get_scrollback_count", "get_scrollback_discarded_count", "get_scrollback_line", "get_scrollback_line_v2",
  "get_scrollback_grapheme", "get_scrollback_hyperlink", "get_viewport_grapheme", "get_viewport_hyperlink",
  "get_row_wraps", "get_scrollback_row_wraps",
  "read_response", "get_title_len", "get_title_ptr", "get_working_directory_len", "get_working_directory_ptr",
  "get_bell_count", "get_clipboard_write_len", "get_clipboard_write_ptr", "get_shell_phase", "get_shell_exit_code",
  "synchronized_output_generation", "get_color_override", "set_theme_colors",
  "find_prompt", "track_position", "resolve_position", "release_position"];

const module = new WebAssembly.Module(readFileSync(process.argv[2]));
const imports = WebAssembly.Module.imports(module);
const exports = new Set(WebAssembly.Module.exports(module).map((e) => e.name));
const missing = REQUIRED.filter((n) => !exports.has(n));
if (imports.length || missing.length) {
  console.error("imports:", imports.map((i) => `${i.module}.${i.name}`), "missing:", missing);
  process.exit(1);
}
console.log(`ok: ${exports.size} exports, 0 imports`);
