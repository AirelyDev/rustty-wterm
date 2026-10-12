//! Kitty graphics: decision gate outcome B.
//!
//! `rustty-vt` compiles the Kitty graphics protocol out on freestanding
//! targets (`options::KITTY_GRAPHICS` is false on `wasm32-unknown-unknown`, the
//! reference's `Options.kittyGraphics` override), so the wasm build ignores
//! every `ESC _ G` command. The `kitty-graphics` feature is therefore off in
//! this crate too, so host tests exercise exactly what ships in the wasm: an
//! image command is swallowed cleanly, with no reply, no panic and no effect on
//! the text that follows it.

use rustty_wterm::Core;

const IMG: &[u8] = b"\x1b_Ga=T,f=24,s=2,v=1,i=7;AQIDBAUG\x1b\\";

fn core() -> Box<Core> {
    Core::new(20, 5, 100, 0, 0, 32 * 1024 * 1024).unwrap()
}

fn replies(c: &mut Core) -> Vec<Vec<u8>> {
    let mut out = vec![];
    let mut b = [0u8; 256];
    loop {
        let n = c.read_response(&mut b);
        if n == 0 {
            return out;
        }
        out.push(b[..n as usize].to_vec());
    }
}

fn first_row_text(c: &mut Core) -> String {
    c.update();
    let mut buf = vec![0u8; 20 * 5 * 16];
    assert_eq!(c.viewport(&mut buf, 16), 100);
    (0..20)
        .map(|x| {
            let cp = u32::from_le_bytes(buf[x * 16..x * 16 + 4].try_into().unwrap());
            char::from_u32(cp).filter(|&ch| ch != '\0').unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_owned()
}

#[test]
fn image_commands_are_ignored_like_the_wasm_build() {
    let mut c = core();
    c.write(IMG);
    c.write(b"\x1b_Ga=q,f=32,s=1,v=1,i=9;AAAAAA==\x1b\\");
    // The wasm build never answers a kitty graphics command; the host build
    // must not either, or host tests would describe a different engine.
    assert_eq!(replies(&mut c), Vec::<Vec<u8>>::new());
    c.write(b"ok");
    assert_eq!(first_row_text(&mut c), "ok");
}

#[test]
fn file_medium_is_never_executed() {
    let mut c = core();
    // path "/etc/passwd"
    c.write(b"\x1b_Ga=T,t=f,f=24,s=1,v=1,i=3;L2V0Yy9wYXNzd2Q=\x1b\\");
    assert_eq!(replies(&mut c), Vec::<Vec<u8>>::new());
    c.write(b"after");
    assert_eq!(first_row_text(&mut c), "after");
}

#[test]
fn oversized_apc_is_swallowed_without_panic() {
    let mut c = core();
    c.write(b"\x1b_Ga=T,f=24,s=1,v=1;");
    let chunk = vec![b'A'; 1024 * 1024];
    for _ in 0..50 {
        c.write(&chunk); // 50 MiB > the Zig layer's 48 MiB cap
    }
    c.write(b"\x1b\\tail");
    assert_eq!(replies(&mut c), Vec::<Vec<u8>>::new());
    assert_eq!(first_row_text(&mut c), "tail");
}

#[test]
fn zero_image_limit_and_plain_text_are_harmless() {
    let mut c = Core::new(20, 5, 100, 0, 0, 0).unwrap();
    c.write(b"just text\r\n");
    c.write(IMG);
    assert_eq!(replies(&mut c), Vec::<Vec<u8>>::new());
    assert_eq!(first_row_text(&mut c), "just text");
}
