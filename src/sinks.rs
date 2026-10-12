//! Host-visible effect sinks: straight ports of the fixed-size structs in
//! `wasm_api.zig` (lines 77-184). Pure data, no VT types except the shell
//! marker enum, so every rule is unit-testable on its own.
//!
//! The two large buffers (the response slots and the clipboard) live on the
//! heap: wasm32 runs with a 128 KiB stack, and building them inline would put
//! ~80 KiB on it while a `Core` is constructed.

pub const RESPONSE_QUEUE_MAX: usize = 256;
pub const RESPONSE_MAX_BYTES: usize = 64;
pub const TITLE_BUFFER_BYTES: usize = 256;
/// Upstream's fixed 2048-byte OSC buffer reserves one byte for the trailing NUL.
pub const DIRECTORY_BUFFER_BYTES: usize = 2047;
pub const CLIPBOARD_MAX_BYTES: usize = 64 * 1024;

/// Fixed-size FIFO of query answers. Drops the newest response when full, so
/// the answers that are kept stay in the order they were produced.
pub struct ResponseQueue {
    slots: Box<[[u8; RESPONSE_MAX_BYTES]; RESPONSE_QUEUE_MAX]>,
    lens: [u8; RESPONSE_QUEUE_MAX],
    head: u16,
    tail: u16,
    count: u16,
}

impl Default for ResponseQueue {
    fn default() -> Self {
        ResponseQueue {
            slots: Box::new([[0; RESPONSE_MAX_BYTES]; RESPONSE_QUEUE_MAX]),
            lens: [0; RESPONSE_QUEUE_MAX],
            head: 0,
            tail: 0,
            count: 0,
        }
    }
}

impl ResponseQueue {
    pub fn push(&mut self, bytes: &[u8]) {
        if bytes.len() > RESPONSE_MAX_BYTES || self.count as usize == RESPONSE_QUEUE_MAX {
            return;
        }
        let tail = self.tail as usize;
        self.slots[tail][..bytes.len()].copy_from_slice(bytes);
        self.lens[tail] = bytes.len() as u8;
        self.tail = ((tail + 1) % RESPONSE_QUEUE_MAX) as u16;
        self.count += 1;
    }

    /// Copy the oldest response into `out` and return its length. Returns 0
    /// when empty, or when `out` is too small (the entry is kept).
    pub fn pop(&mut self, out: &mut [u8]) -> u32 {
        if self.count == 0 {
            return 0;
        }
        let head = self.head as usize;
        let len = self.lens[head] as usize;
        if len > out.len() {
            return 0;
        }
        out[..len].copy_from_slice(&self.slots[head][..len]);
        self.head = ((head + 1) % RESPONSE_QUEUE_MAX) as u16;
        self.count -= 1;
        len as u32
    }
}

/// Window title (OSC 0/2). Matches Ghostty's desktop title limit.
pub struct TitleState {
    pub bytes: [u8; TITLE_BUFFER_BYTES],
    pub len: u16,
    pub changed: bool,
}

impl Default for TitleState {
    fn default() -> Self {
        TitleState {
            bytes: [0; TITLE_BUFFER_BYTES],
            len: 0,
            changed: false,
        }
    }
}

impl TitleState {
    pub fn set(&mut self, title: &[u8]) {
        if title.len() >= self.bytes.len() {
            return;
        }
        self.bytes[..title.len()].copy_from_slice(title);
        self.len = title.len() as u16;
        self.changed = true;
    }
}

/// Latest complete UTF-8 OSC 7 report; never interpreted.
pub struct DirectoryState {
    pub bytes: [u8; DIRECTORY_BUFFER_BYTES],
    pub len: u16,
    pub changed: bool,
}

impl Default for DirectoryState {
    fn default() -> Self {
        DirectoryState {
            bytes: [0; DIRECTORY_BUFFER_BYTES],
            len: 0,
            changed: false,
        }
    }
}

impl DirectoryState {
    pub fn set(&mut self, uri: &[u8]) {
        if uri.len() > self.bytes.len() || core::str::from_utf8(uri).is_err() {
            return;
        }
        if &self.bytes[..self.len as usize] == uri {
            return;
        }
        self.bytes[..uri.len()].copy_from_slice(uri);
        self.len = uri.len() as u16;
        self.changed = true;
    }
}

/// Shell phase as `get_shell_phase` reports it (`enum(u8)` in the Zig layer).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShellPhase {
    #[default]
    Unknown = 0,
    Prompt = 1,
    Input = 2,
    Running = 3,
    Complete = 4,
}

/// An OSC 133 marker, already classified (`fresh_line` never reaches here).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellMarker {
    Prompt,
    Input,
    Running,
    Complete { exit_code: Option<i32> },
}

/// Constant-size shell-integration state. Prompt redraws retain the last
/// reported result; starting a command clears it. Command text is not kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShellState {
    pub phase: ShellPhase,
    pub exit_code: Option<i32>,
    pub changed: bool,
}

impl ShellState {
    pub fn marker(&mut self, marker: ShellMarker) {
        let (phase, exit_code) = match marker {
            ShellMarker::Prompt => (ShellPhase::Prompt, self.exit_code),
            ShellMarker::Input => (ShellPhase::Input, self.exit_code),
            ShellMarker::Running => (ShellPhase::Running, None),
            ShellMarker::Complete { exit_code } => (ShellPhase::Complete, exit_code),
        };
        if self.phase == phase && self.exit_code == exit_code {
            return;
        }
        self.phase = phase;
        self.exit_code = exit_code;
        self.changed = true;
    }
}

/// Latest valid OSC 52 write. Reading the clipboard is never supported.
pub struct ClipboardState {
    pub bytes: Box<[u8]>,
    pub len: u32,
    pub changed: bool,
}

impl Default for ClipboardState {
    fn default() -> Self {
        ClipboardState {
            bytes: vec![0; CLIPBOARD_MAX_BYTES].into_boxed_slice(),
            len: 0,
            changed: false,
        }
    }
}

impl ClipboardState {
    /// Port of `ClipboardState.set`: selector `c` only, never a `?` read,
    /// strict standard base64, at most 64 KiB decoded, valid UTF-8. An invalid
    /// request never disturbs a pending valid one.
    pub fn set(&mut self, kind: u8, encoded: &[u8]) {
        if kind != b'c' || encoded == b"?" {
            return;
        }
        let Some(len) = base64_decoded_len(encoded) else {
            return;
        };
        if len > CLIPBOARD_MAX_BYTES {
            return;
        }
        let mut decoded = vec![0u8; len];
        if base64_decode(encoded, &mut decoded).is_none() {
            return;
        }
        self.set_decoded(&decoded);
    }

    /// The tail of `set` for payloads the VT engine has already decoded.
    pub fn set_decoded(&mut self, decoded: &[u8]) {
        if decoded.len() > CLIPBOARD_MAX_BYTES || core::str::from_utf8(decoded).is_err() {
            return;
        }
        self.bytes[..decoded.len()].copy_from_slice(decoded);
        self.len = decoded.len() as u32;
        self.changed = true;
    }
}

// -- base64 (std.base64.standard: `=` padding required) --------------------

fn base64_value(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    } as u32)
}

/// `Decoder.calcSizeForSlice`: the padded length must be a multiple of 4.
fn base64_decoded_len(src: &[u8]) -> Option<usize> {
    if !src.len().is_multiple_of(4) {
        return None;
    }
    let pad = src.iter().rev().take_while(|&&c| c == b'=').count();
    if pad > 2 {
        return None;
    }
    Some(src.len() / 4 * 3 - pad)
}

/// Strict decode into `out` (exactly `base64_decoded_len` bytes). Rejects
/// characters outside the alphabet, misplaced padding and non-zero trailing
/// bits, as Zig's standard decoder does.
fn base64_decode(src: &[u8], out: &mut [u8]) -> Option<()> {
    let len = base64_decoded_len(src)?;
    if out.len() != len {
        return None;
    }
    let mut o = 0usize;
    for (i, chunk) in src.chunks_exact(4).enumerate() {
        let last = (i + 1) * 4 == src.len();
        let pad = if last {
            chunk.iter().rev().take_while(|&&c| c == b'=').count()
        } else {
            0
        };
        let mut acc = 0u32;
        for &c in &chunk[..4 - pad] {
            acc = (acc << 6) | base64_value(c)?;
        }
        match pad {
            0 => {
                out[o..o + 3].copy_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
                o += 3;
            }
            1 => {
                // 18 bits, 16 used: the low 2 must be zero.
                if acc & 0x3 != 0 {
                    return None;
                }
                out[o..o + 2].copy_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]);
                o += 2;
            }
            _ => {
                // 12 bits, 8 used: the low 4 must be zero.
                if acc & 0xf != 0 {
                    return None;
                }
                out[o] = (acc >> 4) as u8;
                o += 1;
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(s: &[u8]) -> Option<Vec<u8>> {
        let mut out = vec![0; base64_decoded_len(s)?];
        base64_decode(s, &mut out).map(|()| out)
    }

    #[test]
    fn base64_strict() {
        assert_eq!(decode(b"").as_deref(), Some(&b""[..]));
        assert_eq!(decode(b"aGk=").as_deref(), Some(&b"hi"[..]));
        assert_eq!(decode(b"aGVsbG8=").as_deref(), Some(&b"hello"[..]));
        assert_eq!(decode(b"aGVsbA==").as_deref(), Some(&b"hell"[..]));
        assert_eq!(decode(b"aGVs").as_deref(), Some(&b"hel"[..]));
        assert!(decode(b"aGk").is_none()); // unpadded
        assert!(decode(b"aGl=").is_none()); // non-zero trailing bits
        assert!(decode(b"a=k=").is_none()); // padding in the middle
        assert!(decode(b"====").is_none());
        assert!(decode(b"aG!=").is_none());
    }

    #[test]
    fn shell_marker_rules() {
        let mut s = ShellState::default();
        s.marker(ShellMarker::Complete { exit_code: Some(3) });
        s.changed = false;
        s.marker(ShellMarker::Prompt); // keeps the last result
        assert_eq!(
            (s.phase, s.exit_code, s.changed),
            (ShellPhase::Prompt, Some(3), true)
        );
        s.changed = false;
        s.marker(ShellMarker::Prompt);
        assert!(!s.changed);
        s.marker(ShellMarker::Running); // clears it
        assert_eq!(s.exit_code, None);
    }
}
