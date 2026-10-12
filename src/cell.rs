//! Byte layout of one cell as wterm's JS reads it (`parseCell` in
//! `@wterm/ghostty` `wasm-bindings.js`). Pure: no VT types, so it is tested
//! without a terminal.

pub const CELL_BYTES: usize = 16;
pub const CELL_BYTES_V2: usize = 20;

#[derive(Clone, Debug, Default)]
pub struct CellFields {
    pub codepoint: u32,
    pub fg: Option<[u8; 3]>,
    pub bg: Option<[u8; 3]>,
    /// bold=1 faint=2 italic=4 underline=8 blink=16 inverse=32 invisible=64 strike=128
    pub flags: u8,
    /// 0 = continuation of a wide cell, 1 = normal (also spacer head), 2 = wide
    pub width: u8,
    pub has_grapheme: bool,
    pub has_hyperlink: bool,
    pub spacer_head: bool,
    /// 0 none, 1 single, 2 double, 3 curly, 4 dotted, 5 dashed (v2 only)
    pub underline: u8,
    pub underline_rgb: Option<[u8; 3]>,
}

/// Writes `fields` into `out`. `out.len()` must be 16 (v1) or 20 (v2).
/// Any other length is a caller bug; nothing is written (total, no panic).
pub fn encode_cell(f: &CellFields, out: &mut [u8]) {
    if out.len() != CELL_BYTES && out.len() != CELL_BYTES_V2 {
        return;
    }
    let fg = f.fg.unwrap_or([0; 3]);
    let bg = f.bg.unwrap_or([0; 3]);
    out[0..4].copy_from_slice(&f.codepoint.to_le_bytes());
    out[4..7].copy_from_slice(&fg);
    out[7..10].copy_from_slice(&bg);
    out[10] = f.flags;
    out[11] = f.width;
    out[12] = u8::from(f.fg.is_some()) | (u8::from(f.bg.is_some()) << 1);
    out[13] = u8::from(f.has_grapheme)
        | (u8::from(f.has_hyperlink) << 1)
        | (u8::from(f.spacer_head) << 2);
    out[14] = 0;
    out[15] = 0;
    if out.len() == CELL_BYTES_V2 {
        out[16] = f.underline;
        let u = f.underline_rgb.unwrap_or([0; 3]);
        if f.underline_rgb.is_some() {
            out[12] |= 4;
        }
        out[17..20].copy_from_slice(&u);
    }
}
