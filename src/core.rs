use rustty_vt::alloc::Global;
use rustty_vt::color::{DynamicPalette, DynamicRGB, Palette, RGB};
use rustty_vt::kitty::graphics_image::Limits;
use rustty_vt::modes::{Mode, ModePacked};
use rustty_vt::page::{ContentTag, Wide};
use rustty_vt::render::Dirty;
use rustty_vt::render::RenderState;
use rustty_vt::style::{Color, ColorTag};
use rustty_vt::terminal::{Colors, Options, Resize, Terminal};
use rustty_vt::TerminalStream;

use crate::cell::{encode_cell, CellFields};

/// Cursor state as the `get_cursor_*` exports report it.
#[derive(Clone, Copy, Debug)]
pub struct CursorInfo {
    pub row: u32,
    pub col: u32,
    pub visible: bool,
    /// 0 block (incl. hollow), 1 underline, 2 bar
    pub shape: u32,
    pub blinking: bool,
}

fn pack_flags(s: &rustty_vt::style::Style) -> u8 {
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

/// One terminal plus everything the wterm ABI hangs off it. The `Terminal` is
/// boxed so its address is stable: the stream's handler holds a raw pointer to it.
///
/// Field order is load-bearing: fields drop in declaration order, so `stream`
/// (which points into `terminal`) must be declared, and therefore dropped,
/// before `terminal`.
pub struct Core {
    stream: TerminalStream<'static>,
    render: RenderState,
    terminal: Box<Terminal>,
    cols: u16,
    rows: u16,
}

fn rgb(v: u32) -> DynamicRGB {
    DynamicRGB::init(RGB {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    })
}

impl Core {
    pub fn new(
        cols: u16,
        rows: u16,
        max_scrollback: u32,
        fg_rgb: u32,
        bg_rgb: u32,
        image_storage_limit: u32,
    ) -> Option<Box<Core>> {
        if cols == 0 || rows == 0 {
            return None;
        }
        let mut modes = ModePacked::default();
        // `.default_modes = .{ .grapheme_cluster = true }` in wasm_api.zig
        modes.set_mode(Mode::GraphemeCluster, true);
        let opts = Options {
            cols,
            rows,
            max_scrollback_bytes: Some(max_scrollback as usize),
            max_scrollback_lines: None,
            colors: Colors {
                background: rgb(bg_rgb),
                foreground: rgb(fg_rgb),
                cursor: DynamicRGB::UNSET,
                palette: DynamicPalette::default_palette(),
            },
            default_modes: modes,
            default_cursor_style: rustty_vt::screen::CursorStyle::Block,
            default_cursor_blink: Some(false),
            default_xt_checksum: Default::default(),
            kitty_image_storage_limit: image_storage_limit as usize,
            kitty_image_loading_limits: Limits::DIRECT,
        };
        let mut terminal = Box::new(Terminal::init(Global, opts).ok()?);
        // SAFETY: the stream borrows `*terminal`, which lives on the heap behind
        // a Box that is never moved out of or replaced inside `Core`, so the
        // address is stable for the life of `Core`. `stream` is declared before
        // `terminal`, so it drops first and never outlives its referent. The
        // `'static` lifetime is never exposed outside `Core`.
        let stream: TerminalStream<'static> = unsafe { core::mem::transmute(terminal.vt_stream()) };
        Some(Box::new(Core {
            stream,
            render: RenderState::empty(),
            terminal,
            cols,
            rows,
        }))
    }

    pub fn write(&mut self, data: &[u8]) {
        self.stream.next_slice(data);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }
        // Go through the stream's handler so resize side effects (synchronized
        // output release, mode 2048 reports) match the VT path.
        let resized = self.stream.handler.resize(Resize {
            cols,
            rows,
            cell_size_px: None,
        });
        if resized.is_ok() {
            self.cols = cols;
            self.rows = rows;
        }
    }

    pub fn cols(&self) -> u32 {
        self.cols as u32
    }
    pub fn rows(&self) -> u32 {
        self.rows as u32
    }
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }
    pub fn terminal_mut(&mut self) -> &mut Terminal {
        &mut self.terminal
    }

    /// Refresh the render state from the terminal (`update` export).
    pub fn update(&mut self) {
        let _ = self.render.update(Global, &mut self.terminal);
    }

    /// Port of `getViewport`/`encodeCell`. Returns rows*cols, or 0 (writing
    /// nothing) when `out` cannot hold the whole viewport.
    pub fn viewport(&mut self, out: &mut [u8], cell_bytes: usize) -> u32 {
        use crate::cell::{CELL_BYTES, CELL_BYTES_V2};
        if cell_bytes != CELL_BYTES && cell_bytes != CELL_BYTES_V2 {
            return 0;
        }
        let rs = &self.render;
        let rows = rs.rows as usize;
        let cols = rs.cols as usize;
        let Some(need) = rows
            .checked_mul(cols)
            .and_then(|n| n.checked_mul(cell_bytes))
        else {
            return 0;
        };
        if out.len() < need {
            return 0;
        }
        let palette = &rs.colors.palette;
        let mut offset = 0usize;
        for y in 0..rows {
            let Some(row) = rs.row_data.get(y) else {
                // Pad remaining rows with blank cells.
                out[offset..need].fill(0);
                break;
            };
            let cells = row.cells.slice();
            for x in 0..cols {
                let dst = &mut out[offset..offset + cell_bytes];
                offset += cell_bytes;
                let Some(&raw) = cells.raw.get(x) else {
                    dst.fill(0);
                    continue;
                };
                // The style column is only defined for non-default style ids;
                // it is reused across passes, so never read it otherwise.
                let style = if raw.style_id() != 0 {
                    cells.style.get(x).copied().unwrap_or_default()
                } else {
                    rustty_vt::style::Style::default()
                };
                let tag = raw.content_tag();
                let codepoint = match tag {
                    ContentTag::Codepoint | ContentTag::CodepointGrapheme => {
                        raw.content_codepoint()
                    }
                    _ => 0,
                };
                let has_fg = style.fg_color.tag() != ColorTag::None;
                let has_bg_style = style.bg_color.tag() != ColorTag::None;
                let has_bg_cell =
                    matches!(tag, ContentTag::BgColorPalette | ContentTag::BgColorRgb);
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
                let fields = CellFields {
                    codepoint,
                    fg: has_fg.then(|| resolve(style.fg_color, palette)),
                    bg,
                    flags: pack_flags(&style),
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
                };
                encode_cell(&fields, dst);
            }
        }
        (rows * cols) as u32
    }

    /// 0 clean, 1 partial, 2 full.
    pub fn is_dirty(&self) -> u32 {
        match self.render.dirty {
            Dirty::False => 0,
            Dirty::Partial => 1,
            Dirty::Full => 2,
        }
    }

    pub fn is_dirty_row(&self, row: u16) -> bool {
        self.render
            .row_data
            .get(row as usize)
            .is_some_and(|r| r.dirty)
    }

    pub fn clear_dirty(&mut self) {
        self.render.dirty = Dirty::False;
        for r in self.render.row_data.iter_mut() {
            r.dirty = false;
        }
    }

    pub fn cursor(&self) -> CursorInfo {
        use rustty_vt::cursor::Style;
        let c = &self.render.cursor;
        CursorInfo {
            row: c.active.y,
            col: u32::from(c.active.x),
            visible: c.visible,
            shape: match c.visual_style {
                Style::Block | Style::BlockHollow => 0,
                Style::Underline => 1,
                Style::Bar => 2,
            },
            blinking: c.blinking,
        }
    }

    pub fn cursor_keys_app(&self) -> bool {
        self.terminal.modes.get(Mode::CursorKeys)
    }
    pub fn bracketed_paste(&self) -> bool {
        self.terminal.modes.get(Mode::BracketedPaste)
    }
    pub fn using_alt_screen(&self) -> bool {
        self.terminal.screens.active_key != rustty_vt::screen_set::Key::Primary
    }
    pub fn mouse_tracking(&self) -> u32 {
        use rustty_vt::mouse::Event;
        match self.terminal.flags.mouse_event {
            Event::Normal => 1000,
            Event::Button => 1002,
            Event::Any => 1003,
            _ => 0,
        }
    }
    pub fn mouse_sgr(&self) -> bool {
        self.terminal.flags.mouse_format == rustty_vt::mouse::Format::Sgr
    }
    /// Discriminant order x10,utf8,sgr,urxvt,sgr_pixels = 0..4, matching
    /// `mouseEncoding()` in ghostty-core.js.
    pub fn mouse_encoding(&self) -> u32 {
        use rustty_vt::mouse::Format;
        match self.terminal.flags.mouse_format {
            Format::X10 => 0,
            Format::Utf8 => 1,
            Format::Sgr => 2,
            Format::Urxvt => 3,
            Format::SgrPixels => 4,
        }
    }
    pub fn focus_events(&self) -> bool {
        self.terminal.modes.get(Mode::FocusEvent)
    }
    pub fn synchronized_output(&self) -> bool {
        self.terminal.modes.get(Mode::SynchronizedOutput)
    }
    pub fn kitty_keyboard_flags(&self) -> u32 {
        let screen = self.terminal.screens.active;
        if screen.is_null() {
            return 0;
        }
        // SAFETY: `active` points at a screen owned by `terminal.screens`,
        // which outlives this borrow; we only read a Copy field.
        u32::from(unsafe { (*screen).kitty_keyboard.current().int() })
    }
}
