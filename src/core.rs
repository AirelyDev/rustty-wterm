use rustty_vt::alloc::Global;
use rustty_vt::color::{DynamicPalette, DynamicRGB, RGB};
use rustty_vt::kitty::graphics_image::Limits;
use rustty_vt::modes::{Mode, ModePacked};
use rustty_vt::render::Dirty;
use rustty_vt::render::RenderState;
use rustty_vt::terminal::{Colors, Options, Resize, Terminal};
use rustty_vt::TerminalStream;

use crate::cell::encode_cell;
use crate::effects::{self, ActiveGuard, Sinks};
use crate::positions;
use crate::scrollback::{self, cell_fields};

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
    discard: DiscardTracker,
    /// Effect sinks; boxed so the address handed to `ActiveGuard` is stable.
    sinks: Box<Sinks>,
}

/// `PageList.discardedRows` is a counter the wterm Zig patch adds to ghostty;
/// `rustty-vt` has no such counter and is read-only here, so it is derived from
/// page bookkeeping at each write: pages that existed before and are gone now
/// were pruned from the oldest end (`pages_before + serials_issued -
/// pages_after`), each worth a full page of rows. Exact for whole-page prunes
/// (the only kind `grow` performs); pages both created and pruned inside one
/// write are still counted because `page_serial` advances per allocation.
#[derive(Default)]
struct DiscardTracker {
    serial: u64,
    pages: usize,
    discarded: u32,
    primary: bool,
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
        let mut core = Box::new(Core {
            stream,
            render: RenderState::empty(),
            terminal,
            cols,
            rows,
            discard: DiscardTracker::default(),
            sinks: Box::default(),
        });
        effects::install(&mut core.stream.handler.effects);
        core.discard_rebaseline();
        Some(core)
    }

    pub fn write(&mut self, data: &[u8]) {
        let screen = self.screen_identity();
        {
            // The raw pointer is derived from the `Box<Sinks>` and nothing
            // else touches `self.sinks` until the guard drops; `self.stream`
            // is a disjoint field.
            let sinks: *mut Sinks = &mut *self.sinks;
            let _active = ActiveGuard::new(sinks);
            self.stream.next_slice(data);
        }
        // The Zig layer clears tracked positions whenever an action changes
        // the active screen. rustty-vt has no screen-switch effect, so this
        // compares before and after each write (see README "Known gaps").
        if self.screen_identity() != screen {
            self.sinks.positions.clear(&self.terminal.screens);
        }
        self.discard_sample();
    }

    fn screen_identity(&self) -> (*mut rustty_vt::screen::Screen, usize) {
        let s = &self.terminal.screens;
        (s.active, s.generation(s.active_key))
    }

    fn primary_pages(&self) -> Option<(u64, usize, usize)> {
        let screen = self.terminal.screens.active;
        if screen.is_null()
            || self.terminal.screens.active_key != rustty_vt::screen_set::Key::Primary
        {
            return None;
        }
        // SAFETY: `active` points at a screen owned by `terminal.screens`.
        let pages = unsafe { &(*screen).pages };
        let first = pages.pages.first;
        let first_rows = if first.is_null() {
            0
        } else {
            // SAFETY: non-null list node; reads metadata only.
            unsafe { rustty_vt::pagelist::Node::rows(first) as usize }
        };
        Some((pages.page_serial, pages.total_pages(), first_rows))
    }

    fn discard_rebaseline(&mut self) {
        match self.primary_pages() {
            Some((serial, pages, _)) => {
                self.discard.serial = serial;
                self.discard.pages = pages;
                self.discard.primary = true;
            }
            None => self.discard.primary = false,
        }
    }

    fn discard_sample(&mut self) {
        let Some((serial, pages, first_rows)) = self.primary_pages() else {
            self.discard.primary = false;
            return;
        };
        if !self.discard.primary || serial < self.discard.serial {
            // Returned from the alt screen, or the list was rebuilt.
            self.discard_rebaseline();
            return;
        }
        let issued = (serial - self.discard.serial) as usize;
        let pruned = (self.discard.pages + issued).saturating_sub(pages);
        if pruned > 0 {
            let rows = pruned.saturating_mul(first_rows);
            self.discard.discarded = self
                .discard
                .discarded
                .saturating_add(u32::try_from(rows).unwrap_or(u32::MAX));
        }
        self.discard.serial = serial;
        self.discard.pages = pages;
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }
        // Go through the stream's handler so resize side effects (synchronized
        // output release, mode 2048 reports) match the VT path.
        let sinks: *mut Sinks = &mut *self.sinks;
        let resized = {
            let _active = ActiveGuard::new(sinks);
            self.stream.handler.resize(Resize {
                cols,
                rows,
                cell_size_px: None,
            })
        };
        if resized.is_ok() {
            self.cols = cols;
            self.rows = rows;
        }
        self.discard_rebaseline();
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
                let fields = cell_fields(raw, &style, palette);
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

    pub fn scrollback_count(&self) -> u32 {
        scrollback::count(&self.terminal)
    }

    /// Rows pruned from the oldest end of the primary screen's history.
    pub fn scrollback_discarded(&self) -> u32 {
        if self.discard.primary {
            self.discard.discarded
        } else {
            0
        }
    }

    pub fn scrollback_line(
        &self,
        offset: u32,
        out: &mut [u8],
        max_cols: u32,
        cell_bytes: usize,
    ) -> u32 {
        scrollback::line(&self.terminal, offset, out, max_cols, cell_bytes)
    }

    pub fn grapheme_viewport(&self, row: u32, col: u32, out: Option<&mut [u8]>) -> u32 {
        let rs = &self.render;
        if row >= u32::from(rs.rows) || col >= u32::from(rs.cols) {
            return 0;
        }
        let Some(r) = rs.row_data.get(row as usize) else {
            return 0;
        };
        let cells = r.cells.slice();
        let x = col as usize;
        match cells.raw.get(x) {
            Some(raw) if raw.content_tag() == rustty_vt::page::ContentTag::CodepointGrapheme => {
                scrollback::encode_grapheme(raw.content_codepoint(), cells.grapheme(x), out)
            }
            _ => 0,
        }
    }

    pub fn grapheme_scrollback(&self, offset: u32, col: u32, out: Option<&mut [u8]>) -> u32 {
        scrollback::grapheme_history(&self.terminal, offset, col, out)
    }

    pub fn hyperlink_viewport(&self, row: u32, col: u32, out: Option<&mut [u8]>) -> u32 {
        scrollback::hyperlink_viewport(&self.terminal, row, col, out)
    }

    pub fn hyperlink_scrollback(&self, offset: u32, col: u32, out: Option<&mut [u8]>) -> u32 {
        scrollback::hyperlink_history(&self.terminal, offset, col, out)
    }

    pub fn row_wraps(&self, row: u32) -> i32 {
        scrollback::row_wraps(&self.terminal, row)
    }

    pub fn scrollback_row_wraps(&self, offset: u32) -> i32 {
        scrollback::history_row_wraps(&self.terminal, offset)
    }

    // -- Responses and host effects ----------------------------------------

    /// Pop the oldest queued query answer into `out` (0 = none, or `out` too
    /// small; the entry is then kept).
    pub fn read_response(&mut self, out: &mut [u8]) -> u32 {
        self.sinks.queue.pop(out)
    }

    /// The new title, once: reading consumes the change.
    pub fn title(&mut self) -> Option<&[u8]> {
        let t = &mut self.sinks.title;
        if !t.changed {
            return None;
        }
        t.changed = false;
        Some(&t.bytes[..t.len as usize])
    }
    pub fn title_ptr(&self) -> *const u8 {
        self.sinks.title.bytes.as_ptr()
    }

    /// The new OSC 7 report, once. `Some(b"")` clears the host's directory.
    pub fn working_directory(&mut self) -> Option<&[u8]> {
        let d = &mut self.sinks.directory;
        if !d.changed {
            return None;
        }
        d.changed = false;
        Some(&d.bytes[..d.len as usize])
    }
    pub fn working_directory_ptr(&self) -> *const u8 {
        self.sinks.directory.bytes.as_ptr()
    }

    /// Bells since the last call (saturating), then reset to 0.
    pub fn bell_count(&mut self) -> u32 {
        core::mem::take(&mut self.sinks.bell_count)
    }

    /// The latest valid OSC 52 write, once.
    pub fn clipboard_write(&mut self) -> Option<&[u8]> {
        let c = &mut self.sinks.clipboard;
        if !c.changed {
            return None;
        }
        c.changed = false;
        Some(&c.bytes[..c.len as usize])
    }
    pub fn clipboard_write_ptr(&self) -> *const u8 {
        self.sinks.clipboard.bytes.as_ptr()
    }

    /// -1 unchanged, else the phase (0 unknown .. 4 complete); consumes.
    pub fn shell_phase(&mut self) -> i32 {
        let s = &mut self.sinks.shell;
        if !s.changed {
            return -1;
        }
        s.changed = false;
        s.phase as i32
    }

    /// Last command's exit code, NaN when none.
    pub fn shell_exit_code(&self) -> f64 {
        self.sinks.shell.exit_code.map_or(f64::NAN, f64::from)
    }

    /// Bumped on every off-to-on transition of mode 2026.
    pub fn synchronized_output_generation(&self) -> u32 {
        self.sinks.sync_generation
    }

    /// OSC-set default colour override (0 fg, 1 bg, 2 cursor) as 0xRRGGBB, or
    /// -1 when unset/reset so the host keeps following its theme.
    pub fn color_override(&self, target: u8) -> i32 {
        let colors = &self.terminal.colors;
        let rgb = match target {
            0 => colors.foreground.override_,
            1 => colors.background.override_,
            2 => colors.cursor.override_,
            _ => None,
        };
        rgb.map_or(-1, |c| {
            (i32::from(c.r) << 16) | (i32::from(c.g) << 8) | i32::from(c.b)
        })
    }

    /// Host theme: fg, bg, cursor, then palette 0..15 (0xRRGGBB each).
    /// Changes defaults only; OSC overrides are kept.
    pub fn set_theme_colors(&mut self, values: &[u32; 19]) {
        let to_rgb = |v: u32| RGB {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        };
        let gpa = self.terminal.gpa();
        let colors = &mut self.terminal.colors;
        colors.foreground.default = Some(to_rgb(values[0]));
        colors.background.default = Some(to_rgb(values[1]));
        colors.cursor.default = Some(to_rgb(values[2]));
        let mut palette = *colors.palette.original();
        for (i, v) in values[3..].iter().enumerate() {
            palette[i] = to_rgb(*v);
        }
        // Only fails on allocation failure; the defaults above still apply.
        let _ = colors.palette.change_default(gpa, palette);
        self.terminal.flags.dirty.palette = true;
    }

    // -- Prompts and tracked positions -------------------------------------

    pub fn find_prompt(&self, row: u32, direction: i32) -> f64 {
        positions::find_prompt(&self.terminal, row, direction)
    }

    /// Track the screen cell (row, col); 0 when out of range or the table is full.
    pub fn track_position(&mut self, row: u32, col: u32) -> u32 {
        self.sinks.positions.track(&mut self.terminal, row, col)
    }

    /// `row * 65536 + col` in screen coordinates, or -1 when invalid.
    pub fn resolve_position(&self, id: u32) -> f64 {
        self.sinks.positions.resolve(&self.terminal, id)
    }

    pub fn release_position(&mut self, id: u32) {
        self.sinks.positions.release(&self.terminal.screens, id);
    }
}
