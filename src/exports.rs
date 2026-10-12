//! `#[no_mangle] extern "C"` surface. Handles are `usize` pointers to a boxed
//! `Core`, as in wasm_api.zig. Every function is total: bad input returns the
//! Zig layer's sentinel and never panics (a panic aborts the whole module).
use crate::Core;

fn core<'a>(ptr: usize) -> Option<&'a mut Core> {
    if ptr == 0 {
        return None;
    }
    // SAFETY: `ptr` came from `init` (Box::into_raw) and is live until `deinit`.
    Some(unsafe { &mut *(ptr as *mut Core) })
}

#[no_mangle]
pub extern "C" fn init(
    cols: u16,
    rows: u16,
    max_scrollback: u32,
    fg: u32,
    bg: u32,
    img: u32,
) -> usize {
    match Core::new(cols, rows, max_scrollback, fg, bg, img) {
        Some(b) => Box::into_raw(b) as usize,
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn deinit(ptr: usize) {
    if ptr != 0 {
        // SAFETY: matches the Box::into_raw in `init`; JS calls this once.
        drop(unsafe { Box::from_raw(ptr as *mut Core) });
    }
}

#[no_mangle]
pub extern "C" fn resize(ptr: usize, cols: u16, rows: u16) {
    if let Some(c) = core(ptr) {
        c.resize(cols, rows);
    }
}

#[no_mangle]
pub extern "C" fn write(ptr: usize, data: *const u8, len: u32) {
    let Some(c) = core(ptr) else { return };
    if data.is_null() {
        return;
    }
    // SAFETY: JS passes a buffer from `alloc_buffer` of at least `len` bytes.
    c.write(unsafe { core::slice::from_raw_parts(data, len as usize) });
}

#[no_mangle]
pub extern "C" fn get_cols(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.cols())
}
#[no_mangle]
pub extern "C" fn get_rows(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.rows())
}

#[no_mangle]
pub extern "C" fn alloc_buffer(len: u32) -> usize {
    let mut v: Vec<u8> = Vec::new();
    if v.try_reserve_exact(len as usize).is_err() {
        return 0;
    }
    v.resize(len as usize, 0);
    let b = v.into_boxed_slice();
    Box::into_raw(b) as *mut u8 as usize
}

#[no_mangle]
pub extern "C" fn free_buffer(ptr: usize, len: u32) {
    if ptr != 0 {
        // SAFETY: `ptr`/`len` are exactly what `alloc_buffer` returned/received,
        // i.e. a `Box<[u8]>` of `len` bytes from `Box::into_raw`.
        drop(unsafe {
            Box::from_raw(core::ptr::slice_from_raw_parts_mut(
                ptr as *mut u8,
                len as usize,
            ))
        });
    }
}

// -- Viewport, dirty tracking, cursor, modes -------------------------------

#[no_mangle]
pub extern "C" fn update(ptr: usize) {
    if let Some(c) = core(ptr) {
        c.update();
    }
}

fn viewport_into(ptr: usize, buf: *mut u8, cell_bytes: usize) -> u32 {
    let Some(c) = core(ptr) else { return 0 };
    if buf.is_null() {
        return 0;
    }
    let n = (c.cols() as usize) * (c.rows() as usize) * cell_bytes;
    // SAFETY: JS allocated cols*rows*cell_bytes bytes via alloc_buffer for this call.
    c.viewport(
        unsafe { core::slice::from_raw_parts_mut(buf, n) },
        cell_bytes,
    )
}

#[no_mangle]
pub extern "C" fn get_viewport(ptr: usize, buf: *mut u8) -> u32 {
    viewport_into(ptr, buf, crate::cell::CELL_BYTES)
}

#[no_mangle]
pub extern "C" fn get_viewport_v2(ptr: usize, buf: *mut u8) -> u32 {
    viewport_into(ptr, buf, crate::cell::CELL_BYTES_V2)
}

#[no_mangle]
pub extern "C" fn is_dirty(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.is_dirty())
}
#[no_mangle]
pub extern "C" fn is_dirty_row(ptr: usize, row: u16) -> u32 {
    core(ptr).map_or(0, |c| u32::from(c.is_dirty_row(row)))
}
#[no_mangle]
pub extern "C" fn clear_dirty(ptr: usize) {
    if let Some(c) = core(ptr) {
        c.clear_dirty();
    }
}

#[no_mangle]
pub extern "C" fn get_cursor_row(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.cursor().row)
}
#[no_mangle]
pub extern "C" fn get_cursor_col(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.cursor().col)
}
#[no_mangle]
pub extern "C" fn get_cursor_visible(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| u32::from(c.cursor().visible))
}
#[no_mangle]
pub extern "C" fn get_cursor_shape(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.cursor().shape)
}
#[no_mangle]
pub extern "C" fn get_cursor_blinking(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| u32::from(c.cursor().blinking))
}

#[no_mangle]
pub extern "C" fn cursor_keys_app(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| u32::from(c.cursor_keys_app()))
}
#[no_mangle]
pub extern "C" fn bracketed_paste(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| u32::from(c.bracketed_paste()))
}
#[no_mangle]
pub extern "C" fn using_alt_screen(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| u32::from(c.using_alt_screen()))
}
#[no_mangle]
pub extern "C" fn mouse_tracking(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.mouse_tracking())
}
#[no_mangle]
pub extern "C" fn mouse_sgr(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| u32::from(c.mouse_sgr()))
}
#[no_mangle]
pub extern "C" fn mouse_encoding(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.mouse_encoding())
}
#[no_mangle]
pub extern "C" fn focus_events(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| u32::from(c.focus_events()))
}
#[no_mangle]
pub extern "C" fn synchronized_output(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| u32::from(c.synchronized_output()))
}
#[no_mangle]
pub extern "C" fn kitty_keyboard_flags(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.kitty_keyboard_flags())
}

// -- Scrollback, graphemes, hyperlinks, wrap flags -------------------------

#[no_mangle]
pub extern "C" fn get_scrollback_count(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.scrollback_count())
}
#[no_mangle]
pub extern "C" fn get_scrollback_discarded_count(ptr: usize) -> u32 {
    core(ptr).map_or(0, |c| c.scrollback_discarded())
}

fn scrollback_into(ptr: usize, offset: u32, buf: *mut u8, max_cols: u32, cell_bytes: usize) -> u32 {
    let Some(c) = core(ptr) else { return 0 };
    if buf.is_null() {
        return 0;
    }
    let Some(n) = (max_cols as usize).checked_mul(cell_bytes) else {
        return 0;
    };
    // SAFETY: JS allocated max_cols*cell_bytes bytes via alloc_buffer for this call.
    c.scrollback_line(
        offset,
        unsafe { core::slice::from_raw_parts_mut(buf, n) },
        max_cols,
        cell_bytes,
    )
}

#[no_mangle]
pub extern "C" fn get_scrollback_line(ptr: usize, offset: u32, buf: *mut u8, max_cols: u32) -> u32 {
    scrollback_into(ptr, offset, buf, max_cols, crate::cell::CELL_BYTES)
}
#[no_mangle]
pub extern "C" fn get_scrollback_line_v2(
    ptr: usize,
    offset: u32,
    buf: *mut u8,
    max_cols: u32,
) -> u32 {
    scrollback_into(ptr, offset, buf, max_cols, crate::cell::CELL_BYTES_V2)
}

/// `buf_ptr == 0` means a length query.
fn out_slice<'a>(buf_ptr: usize, buf_len: u32) -> Option<&'a mut [u8]> {
    if buf_ptr == 0 {
        return None;
    }
    // SAFETY: JS passes a buffer of `buf_len` bytes from `alloc_buffer`.
    Some(unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_len as usize) })
}

#[no_mangle]
pub extern "C" fn get_scrollback_grapheme(
    ptr: usize,
    offset: u32,
    col: u32,
    buf_ptr: usize,
    buf_len: u32,
) -> u32 {
    core(ptr).map_or(0, |c| {
        c.grapheme_scrollback(offset, col, out_slice(buf_ptr, buf_len))
    })
}
#[no_mangle]
pub extern "C" fn get_scrollback_hyperlink(
    ptr: usize,
    offset: u32,
    col: u32,
    buf_ptr: usize,
    buf_len: u32,
) -> u32 {
    core(ptr).map_or(0, |c| {
        c.hyperlink_scrollback(offset, col, out_slice(buf_ptr, buf_len))
    })
}
#[no_mangle]
pub extern "C" fn get_viewport_grapheme(
    ptr: usize,
    row: u32,
    col: u32,
    buf_ptr: usize,
    buf_len: u32,
) -> u32 {
    core(ptr).map_or(0, |c| {
        c.grapheme_viewport(row, col, out_slice(buf_ptr, buf_len))
    })
}
#[no_mangle]
pub extern "C" fn get_viewport_hyperlink(
    ptr: usize,
    row: u32,
    col: u32,
    buf_ptr: usize,
    buf_len: u32,
) -> u32 {
    core(ptr).map_or(0, |c| {
        c.hyperlink_viewport(row, col, out_slice(buf_ptr, buf_len))
    })
}

#[no_mangle]
pub extern "C" fn get_row_wraps(ptr: usize, row: u32) -> i32 {
    core(ptr).map_or(-1, |c| c.row_wraps(row))
}
#[no_mangle]
pub extern "C" fn get_scrollback_row_wraps(ptr: usize, offset: u32) -> i32 {
    core(ptr).map_or(-1, |c| c.scrollback_row_wraps(offset))
}
