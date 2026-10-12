//! Scrollback / viewport row readers over `PageList` pins, plus the grapheme
//! and hyperlink encoders. Ports of `wasm_api.zig` lines 1013-1113 and
//! 1410-1526.
//!
//! Every function is total: out-of-range inputs return 0 / -1, never panic.

use rustty_vt::color::{Palette, RGB};
use rustty_vt::page::{Cell, ContentTag, Wide};
use rustty_vt::pagelist::pin::CellSubset;
use rustty_vt::pagelist::{Node, Pin};
use rustty_vt::point::Tag;
use rustty_vt::style::{Color, ColorTag, Style};
use rustty_vt::terminal::Terminal;

use crate::cell::{encode_cell, CellFields, CELL_BYTES, CELL_BYTES_V2};

fn pack_flags(s: &Style) -> u8 {
    use rustty_vt::sgr::Underline;
    let f = &s.flags;
    u8::from(f.bold)
        | (u8::from(f.faint) << 1)
        | (u8::from(f.italic) << 2)
        | (u8::from(f.underline != Underline::None) << 3)
        | (u8::from(f.blink) << 4)
        | (u8::from(f.inverse) << 5)
        | (u8::from(f.invisible) << 6)
        | (u8::from(f.strikethrough) << 7)
}

fn resolve(c: Color, palette: &Palette) -> [u8; 3] {
    let rgb = match c.tag() {
        ColorTag::None => RGB { r: 0, g: 0, b: 0 },
        ColorTag::Palette => palette[c.palette() as usize],
        ColorTag::Rgb => c.rgb(),
    };
    [rgb.r, rgb.g, rgb.b]
}

/// Port of `encodeCell`'s field derivation, shared by the viewport and
/// scrollback paths. `style` must already be the default style for cells whose
/// style id is 0.
pub(crate) fn cell_fields(raw: Cell, style: &Style, palette: &Palette) -> CellFields {
    let tag = raw.content_tag();
    let codepoint = match tag {
        ContentTag::Codepoint | ContentTag::CodepointGrapheme => raw.content_codepoint(),
        _ => 0,
    };
    let has_fg = style.fg_color.tag() != ColorTag::None;
    let has_bg_style = style.bg_color.tag() != ColorTag::None;
    let has_bg_cell = matches!(tag, ContentTag::BgColorPalette | ContentTag::BgColorRgb);
    let bg = if has_bg_cell {
        let c = match tag {
            ContentTag::BgColorPalette => palette[raw.content_color_palette() as usize],
            _ => {
                let c = raw.content_color_rgb();
                RGB {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                }
            }
        };
        Some([c.r, c.g, c.b])
    } else if has_bg_style {
        Some(resolve(style.bg_color, palette))
    } else {
        None
    };
    let wide = raw.wide();
    CellFields {
        codepoint,
        fg: has_fg.then(|| resolve(style.fg_color, palette)),
        bg,
        flags: pack_flags(style),
        width: match wide {
            Wide::Narrow | Wide::SpacerHead => 1,
            Wide::Wide => 2,
            Wide::SpacerTail => 0,
        },
        has_grapheme: tag == ContentTag::CodepointGrapheme,
        has_hyperlink: raw.hyperlink(),
        spacer_head: wide == Wide::SpacerHead,
        underline: style.flags.underline as u8,
        underline_rgb: style.underline_color(palette).map(|c| [c.r, c.g, c.b]),
    }
}

/// Total retained rows minus visible rows (0 if not greater).
pub(crate) fn count(term: &Terminal) -> u32 {
    let screen = term.screens.active;
    if screen.is_null() {
        return 0;
    }
    // SAFETY: `active` points at a screen owned by `term.screens`, alive for
    // the borrow of `term`; we only read the page list.
    let pages = unsafe { &(*screen).pages };
    let total = pages.total_rows;
    let visible = term.rows as usize;
    if total <= visible {
        0
    } else {
        u32::try_from(total - visible).unwrap_or(u32::MAX)
    }
}

/// Pin `offset + 1` rows above the active top-left. The add is checked.
fn history_pin(term: &Terminal, offset: u32) -> Option<Pin> {
    let rows_up = (offset as usize).checked_add(1)?;
    active_top(term)?.up(rows_up)
}

fn active_top(term: &Terminal) -> Option<Pin> {
    let screen = term.screens.active;
    if screen.is_null() {
        return None;
    }
    // SAFETY: see `count`.
    let pages = unsafe { &(*screen).pages };
    Some(pages.get_top_left(Tag::Active))
}

fn viewport_pin(term: &Terminal, row: u32) -> Option<Pin> {
    if row >= term.rows as u32 {
        return None;
    }
    active_top(term)?.down(row as usize)
}

/// Cells of the row `pin` is on.
fn row_cells<'a>(pin: &Pin) -> &'a [Cell] {
    // SAFETY: `cells(All)` returns the live cell slice of the pin's row; the
    // page is not mutated while the caller holds the `&Terminal` borrow.
    unsafe { &*pin.cells(CellSubset::All) }
}

/// Port of `getScrollbackLine`. Returns cells written (<= `max_cols`).
pub(crate) fn line(
    term: &Terminal,
    offset: u32,
    out: &mut [u8],
    max_cols: u32,
    cell_bytes: usize,
) -> u32 {
    if cell_bytes != CELL_BYTES && cell_bytes != CELL_BYTES_V2 {
        return 0;
    }
    let Some(pin) = history_pin(term, offset) else {
        return 0;
    };
    // History rows use the terminal's palette, not the render state's.
    let palette = &term.colors.palette.current;
    let cells = row_cells(&pin);
    let count = cells
        .len()
        .min(max_cols as usize)
        .min(out.len() / cell_bytes);
    for (i, raw) in cells[..count].iter().enumerate() {
        let style = pin.style(raw);
        let fields = cell_fields(*raw, &style, palette);
        encode_cell(&fields, &mut out[i * cell_bytes..(i + 1) * cell_bytes]);
    }
    count as u32
}

/// Port of `encodeGrapheme`.
pub(crate) fn encode_grapheme(base: u32, extras: &[u32], out: Option<&mut [u8]>) -> u32 {
    if extras.is_empty() {
        return 0;
    }
    let mut required = 0usize;
    for cp in core::iter::once(&base).chain(extras) {
        let Some(c) = char::from_u32(*cp) else {
            return 0;
        };
        required += c.len_utf8();
    }
    let Some(out) = out else {
        return required as u32;
    };
    if required > out.len() {
        return required as u32;
    }
    let mut off = 0;
    for cp in core::iter::once(&base).chain(extras) {
        let Some(c) = char::from_u32(*cp) else {
            return 0;
        };
        off += c.encode_utf8(&mut out[off..]).len();
    }
    off as u32
}

pub(crate) fn grapheme_history(
    term: &Terminal,
    offset: u32,
    col: u32,
    out: Option<&mut [u8]>,
) -> u32 {
    let Some(pin) = history_pin(term, offset) else {
        return 0;
    };
    let cells = row_cells(&pin);
    let Some(cell) = cells.get(col as usize) else {
        return 0;
    };
    if cell.content_tag() != ContentTag::CodepointGrapheme {
        return 0;
    }
    let Some(extras) = pin.grapheme(cell) else {
        return 0;
    };
    encode_grapheme(cell.content_codepoint(), extras, out)
}

/// Port of `encodeHyperlink`: `uri \0 explicit-id \0 implicit-id`.
fn encode_hyperlink(pin: &Pin, col: u32, out: Option<&mut [u8]>) -> u32 {
    use rustty_vt::hyperlink::PageEntryIdTag;
    let cells = row_cells(pin);
    let Some(cell) = cells.get(col as usize) else {
        return 0;
    };
    if !cell.hyperlink() {
        return 0;
    }
    // SAFETY: the pin's node is live; `Node::page` restores a discarded page.
    let page = unsafe { &*Node::page(pin.node) };
    let Some(id) = page.lookup_hyperlink(cell) else {
        return 0;
    };
    let base = page.memory_ptr();
    // SAFETY: `id` came from this page's hyperlink map, so it is a live set
    // entry, and its offsets point into this page's memory.
    let entry = unsafe { *page.hyperlink_set.get(base, id) };
    // SAFETY: as above.
    let uri: &[u8] = unsafe { entry.uri.slice(base) };
    let implicit;
    let (explicit_id, implicit_id): (&[u8], &[u8]) = match entry.id.tag() {
        // SAFETY: tag checked; slice offsets point into this page's memory.
        PageEntryIdTag::Explicit => (unsafe { entry.id.explicit().slice(base) }, b""),
        PageEntryIdTag::Implicit => {
            implicit = entry.id.implicit().to_string();
            (b"", implicit.as_bytes())
        }
    };
    let required = uri.len() + explicit_id.len() + implicit_id.len() + 2;
    let Some(out) = out else {
        return required as u32;
    };
    if required > out.len() {
        return required as u32;
    }
    let mut off = 0;
    out[off..off + uri.len()].copy_from_slice(uri);
    off += uri.len();
    out[off] = 0;
    off += 1;
    out[off..off + explicit_id.len()].copy_from_slice(explicit_id);
    off += explicit_id.len();
    out[off] = 0;
    off += 1;
    out[off..off + implicit_id.len()].copy_from_slice(implicit_id);
    required as u32
}

pub(crate) fn hyperlink_viewport(
    term: &Terminal,
    row: u32,
    col: u32,
    out: Option<&mut [u8]>,
) -> u32 {
    match viewport_pin(term, row) {
        Some(pin) => encode_hyperlink(&pin, col, out),
        None => 0,
    }
}

pub(crate) fn hyperlink_history(
    term: &Terminal,
    offset: u32,
    col: u32,
    out: Option<&mut [u8]>,
) -> u32 {
    match history_pin(term, offset) {
        Some(pin) => encode_hyperlink(&pin, col, out),
        None => 0,
    }
}

fn wrap_flags(pin: &Pin) -> i32 {
    // SAFETY: `row_and_cell` returns a live row pointer for a valid pin.
    let row = unsafe { *pin.row_and_cell().row };
    i32::from(row.wrap()) | (i32::from(row.wrap_continuation()) << 1)
}

pub(crate) fn row_wraps(term: &Terminal, row: u32) -> i32 {
    viewport_pin(term, row).map_or(-1, |p| wrap_flags(&p))
}

pub(crate) fn history_row_wraps(term: &Terminal, offset: u32) -> i32 {
    history_pin(term, offset).map_or(-1, |p| wrap_flags(&p))
}
