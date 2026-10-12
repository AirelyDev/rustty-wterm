use rustty_vt::alloc::Global;
use rustty_vt::color::{DynamicPalette, DynamicRGB, RGB};
use rustty_vt::kitty::graphics_image::Limits;
use rustty_vt::modes::{Mode, ModePacked};
use rustty_vt::render::RenderState;
use rustty_vt::terminal::{Colors, Options, Resize, Terminal};
use rustty_vt::TerminalStream;

/// One terminal plus everything the wterm ABI hangs off it. The `Terminal` is
/// boxed so its address is stable: the stream's handler holds a raw pointer to it.
///
/// Field order is load-bearing: fields drop in declaration order, so `stream`
/// (which points into `terminal`) must be declared, and therefore dropped,
/// before `terminal`.
pub struct Core {
    stream: TerminalStream<'static>,
    // Consumed by the render exports (Task 2).
    #[allow(dead_code)]
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
}
