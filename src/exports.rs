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
