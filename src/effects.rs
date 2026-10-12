//! Routes `rustty-vt` stream effects into the wterm sinks.
//!
//! The Zig layer wraps Ghostty's readonly handler in its own `ResponseHandler`
//! (wasm_api.zig 186-482) holding pointers to the sinks. `rustty-vt` instead
//! exposes `stream_terminal::Effects`: plain `fn` pointers with no user-data
//! argument. Callbacks therefore reach the sinks through one module-level
//! "active sinks" pointer that `Core::write` sets for the duration of
//! `stream.next_slice` with an [`ActiveGuard`] (its `Drop` restores the
//! previous value, so the pointer can never outlive the borrow it came from).
//!
//! On wasm32-unknown-unknown (built without the atomics feature) there is
//! exactly one thread, so a `static mut` is sound: no other code can observe
//! the pointer while a write is in progress. Host builds (`cargo test` runs
//! tests on parallel threads) use a `thread_local!` instead, so two cores
//! written on two threads never see each other's sinks.

use rustty_vt::clipboard;
use rustty_vt::device_attributes::{Attributes, ConformanceLevel, Feature, Primary};
use rustty_vt::screen_set::Key;
use rustty_vt::stream_terminal::{Effects, Handler, SemanticPrompt, SemanticPromptKind};

use crate::positions::PositionTracker;
use crate::sinks::{
    ClipboardState, DirectoryState, ResponseQueue, ShellMarker, ShellState, TitleState,
};

/// Everything the effect callbacks write into. Boxed inside `Core` so its
/// address is stable while the active pointer refers to it.
#[derive(Default)]
pub(crate) struct Sinks {
    pub queue: ResponseQueue,
    pub title: TitleState,
    pub directory: DirectoryState,
    pub clipboard: ClipboardState,
    pub shell: ShellState,
    pub bell_count: u32,
    pub sync_generation: u32,
    pub positions: PositionTracker,
}

#[cfg(target_arch = "wasm32")]
mod active {
    use super::Sinks;

    static mut ACTIVE: *mut Sinks = core::ptr::null_mut();

    pub(super) fn get() -> *mut Sinks {
        // SAFETY: wasm32-unknown-unknown is single-threaded; no reference to
        // the static is formed, only a by-value read of the pointer.
        unsafe { core::ptr::addr_of!(ACTIVE).read() }
    }

    pub(super) fn set(p: *mut Sinks) {
        // SAFETY: as in `get`; a by-value write through a raw pointer.
        unsafe { core::ptr::addr_of_mut!(ACTIVE).write(p) }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod active {
    use super::Sinks;
    use core::cell::Cell;

    std::thread_local! {
        static ACTIVE: Cell<*mut Sinks> = const { Cell::new(core::ptr::null_mut()) };
    }

    pub(super) fn get() -> *mut Sinks {
        ACTIVE.with(Cell::get)
    }

    pub(super) fn set(p: *mut Sinks) {
        ACTIVE.with(|a| a.set(p));
    }
}

/// Makes `sinks` the target of effect callbacks until dropped.
pub(crate) struct ActiveGuard {
    prev: *mut Sinks,
}

impl ActiveGuard {
    /// `sinks` must stay valid, and must not be accessed through any other
    /// path, until the guard is dropped.
    pub(crate) fn new(sinks: *mut Sinks) -> ActiveGuard {
        let prev = active::get();
        active::set(sinks);
        ActiveGuard { prev }
    }
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        active::set(self.prev);
    }
}

fn with_sinks(f: impl FnOnce(&mut Sinks)) {
    let p = active::get();
    if p.is_null() {
        return;
    }
    // SAFETY: a non-null active pointer was installed by an `ActiveGuard`
    // whose creator guarantees exclusive access to the sinks until the guard
    // drops (see `Core::write`); callbacks never nest, so this is the only
    // live reference.
    f(unsafe { &mut *p });
}

type H = Handler<'static>;

/// Install the callbacks. Effects the Zig layer does not answer stay `None`
/// (`color_scheme`, `enquiry`, `size`, `xtversion`, clipboard reads, ...):
/// an unanswered query must stay unanswered.
pub(crate) fn install(effects: &mut Effects<'static>) {
    effects.write_pty = Some(write_pty);
    effects.bell = Some(bell);
    effects.title_changed = Some(title_changed);
    effects.pwd_changed = Some(pwd_changed);
    effects.clipboard_write = Some(clipboard_write);
    effects.device_attributes = Some(device_attributes);
    effects.semantic_prompt = Some(semantic_prompt);
    effects.render_hold = Some(render_hold);
    effects.reset = Some(reset);
}

// -- Responses ---------------------------------------------------------------

/// `rustty-vt` formats every reply itself once `write_pty` is set (DSR/CPR,
/// DECRPM, kitty keyboard, OSC 10/11, kitty graphics), so this only routes the
/// bytes; nothing is re-formatted here. It also answers queries the Zig layer
/// leaves unanswered (secondary/tertiary DA, DECRQSS, XTGETTCAP, XTVERSION,
/// OSC 4/12/21 colour queries, OSC 5522, visibility/colour-scheme reports).
/// Those are dropped so the two engines produce the same reply stream.
fn write_pty(h: &mut H, data: &[u8]) {
    if is_cpr(data) {
        // rustty-vt reports CPR relative to the scrolling region under origin
        // mode (DECOM); the Zig layer always reports the absolute cursor
        // (wasm_api.zig 341-351). Re-encode from the absolute position.
        // SAFETY: as in `title_changed`; the screen pointer is the live
        // active screen and only Copy fields are read.
        let (x, y) = unsafe {
            let cursor = &(*(*h.terminal).screens.active).cursor;
            (u32::from(cursor.x), u32::from(cursor.y))
        };
        let reply = format!("\x1b[{};{}R", y + 1, x + 1);
        with_sinks(|s| s.queue.push(reply.as_bytes()));
        return;
    }
    with_sinks(|s| route_reply(&mut s.queue, data));
}

/// `CSI row ; col R`
fn is_cpr(data: &[u8]) -> bool {
    data.strip_prefix(b"\x1b[")
        .and_then(digits)
        .and_then(|r| r.strip_prefix(b";"))
        .and_then(digits)
        == Some(b"R")
}

pub(crate) fn route_reply(queue: &mut ResponseQueue, data: &[u8]) {
    if data.starts_with(b"\x1b]") {
        // One OSC colour operation can carry several queries; rustty-vt
        // writes their reports as one chunk, the Zig layer queues one entry
        // per report and only answers the dynamic foreground/background.
        for report in split_osc(data) {
            if report.starts_with(b"\x1b]10;") || report.starts_with(b"\x1b]11;") {
                queue.push(report);
            }
        }
        return;
    }
    if zig_answers(data) {
        queue.push(data);
    }
}

/// Split a run of OSC sequences at each `ESC ]` introducer.
fn split_osc(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = data;
    core::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = rest
            .windows(2)
            .skip(1)
            .position(|w| w == b"\x1b]")
            .map_or(rest.len(), |i| i + 1);
        let (head, tail) = rest.split_at(end);
        rest = tail;
        Some(head)
    })
}

fn digits(s: &[u8]) -> Option<&[u8]> {
    let n = s.iter().take_while(|c| c.is_ascii_digit()).count();
    (n > 0).then(|| &s[n..])
}

/// The non-OSC reply shapes `ResponseHandler` produces.
fn zig_answers(data: &[u8]) -> bool {
    // DA1 (only the primary query), DSR 5.
    if data == b"\x1b[?1;2c" || data == b"\x1b[0n" {
        return true;
    }
    // Kitty graphics responses (wasm_api.zig handleKittyCommand).
    if data.starts_with(b"\x1b_G") {
        return true;
    }
    let Some(body) = data.strip_prefix(b"\x1b[") else {
        return false;
    };
    if is_cpr(data) {
        return true;
    }
    // DECRPM: CSI [?] mode ; state $ y
    let mode = body.strip_prefix(b"?").unwrap_or(body);
    if let Some(r) = digits(mode).and_then(|r| r.strip_prefix(b";")) {
        if r.len() == 3 && r[0].is_ascii_digit() && &r[1..] == b"$y" {
            return true;
        }
    }
    // Kitty keyboard query: CSI ? flags u
    if let Some(r) = body.strip_prefix(b"?") {
        if digits(r) == Some(b"u") {
            return true;
        }
    }
    false
}

/// VT100 with the advanced video option: exactly `ESC [ ? 1 ; 2 c`. Secondary
/// and tertiary encodings are dropped by `route_reply`.
fn device_attributes(_: &mut H) -> Attributes {
    Attributes {
        primary: Primary {
            conformance_level: ConformanceLevel::VT100,
            features: &[Feature::Printer],
        },
        ..Attributes::default()
    }
}

// -- Host state --------------------------------------------------------------

fn bell(_: &mut H) {
    with_sinks(|s| s.bell_count = s.bell_count.saturating_add(1));
}

fn title_changed(h: &mut H) {
    // SAFETY: effects run with no `&mut Terminal` live (stream_terminal's
    // `render_hold` rule); this shared borrow ends before we return.
    let term = unsafe { &*h.terminal };
    let title = term.get_title().unwrap_or(b"");
    with_sinks(|s| s.title.set(title));
}

fn pwd_changed(h: &mut H) {
    // SAFETY: as in `title_changed`.
    let term = unsafe { &*h.terminal };
    let pwd = term.get_pwd().unwrap_or(b"");
    with_sinks(|s| s.directory.set(pwd));
}

/// OSC 52 writes arrive already base64-decoded. Only the standard clipboard
/// (`c`, and selectors rustty-vt maps to it) and only OSC 52's shape
/// (no reply context, no name, one text/plain item) are kept; Kitty OSC 5522
/// writes are not part of the Zig layer.
fn clipboard_write(_: &mut H, write: clipboard::Write) {
    if write.location != clipboard::Location::Standard
        || !write.reply_ctx.is_null()
        || !write.name.is_empty()
        || write.contents.len() > 1
    {
        return;
    }
    let data = write.contents.first().map_or(&b""[..], |c| c.data);
    with_sinks(|s| s.clipboard.set_decoded(data));
}

fn semantic_prompt(h: &mut H, sp: SemanticPrompt<'_>) {
    // SAFETY: as in `title_changed`.
    if unsafe { (*h.terminal).screens.active_key } != Key::Primary {
        return;
    }
    let marker = match sp.kind {
        SemanticPromptKind::PromptStart => ShellMarker::Prompt,
        SemanticPromptKind::InputStart => ShellMarker::Input,
        SemanticPromptKind::OutputStart => ShellMarker::Running,
        SemanticPromptKind::CommandEnd => ShellMarker::Complete {
            exit_code: sp.exit_code,
        },
        SemanticPromptKind::Invalid => return,
    };
    with_sinks(|s| s.shell.marker(marker));
}

/// rustty-vt calls `render_hold(true)` exactly on the off-to-on transition of
/// mode 2026, from both set_mode and restore_mode (XTRESTORE).
fn render_hold(_: &mut H, held: bool) {
    if held {
        with_sinks(|s| s.sync_generation = s.sync_generation.wrapping_add(1));
    }
}

/// Full reset (RIS). Runs after the terminal has reset; the alternate screen
/// is already freed, which `PositionTracker` detects through its generation.
fn reset(h: &mut H) {
    // SAFETY: as in `title_changed`.
    let term = unsafe { &*h.terminal };
    with_sinks(|s| {
        s.positions.clear(&term.screens);
        s.shell = ShellState {
            changed: true,
            ..ShellState::default()
        };
        s.directory = DirectoryState {
            changed: true,
            ..DirectoryState::default()
        };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routed(data: &[u8]) -> Vec<Vec<u8>> {
        let mut q = ResponseQueue::default();
        route_reply(&mut q, data);
        let mut out = vec![];
        let mut b = [0u8; 64];
        loop {
            let n = q.pop(&mut b);
            if n == 0 {
                break;
            }
            out.push(b[..n as usize].to_vec());
        }
        out
    }

    #[test]
    fn keeps_zig_reply_shapes() {
        for r in [
            &b"\x1b[?1;2c"[..],
            b"\x1b[0n",
            b"\x1b[3;7R",
            b"\x1b[?2004;1$y",
            b"\x1b[4;2$y",
            b"\x1b[?9999;0$y",
            b"\x1b[?1u",
            b"\x1b_Gi=1;OK\x1b\\",
        ] {
            assert_eq!(routed(r), vec![r.to_vec()], "{r:?}");
        }
    }

    #[test]
    fn drops_replies_the_zig_layer_never_sends() {
        for r in [
            &b"\x1b[>1;10;0c"[..],
            b"\x1bP!|00000000\x1b\\",
            b"\x1bP>|librustty\x1b\\",
            b"\x1bP1$r0m\x1b\\",
            b"\x1b]4;1;rgb:cdcd/0000/0000\x07",
            b"\x1b]12;rgb:ffff/ffff/ffff\x07",
            b"\x1b[?997;1n",
            b"\x1b[8;24;80t",
        ] {
            assert!(routed(r).is_empty(), "{r:?}");
        }
    }

    #[test]
    fn splits_combined_osc_colour_reports() {
        assert_eq!(
            routed(b"\x1b]10;rgb:1111/1111/1111\x1b\\\x1b]12;rgb:0/0/0\x1b\\\x1b]11;rgb:2222/2222/2222\x1b\\"),
            vec![
                b"\x1b]10;rgb:1111/1111/1111\x1b\\".to_vec(),
                b"\x1b]11;rgb:2222/2222/2222\x1b\\".to_vec(),
            ]
        );
    }
}
