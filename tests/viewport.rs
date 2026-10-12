use rustty_wterm::{cell::CELL_BYTES_V2, Core};

fn viewport(core: &mut Core) -> Vec<u8> {
    core.update();
    let mut buf = vec![0u8; (core.cols() * core.rows()) as usize * CELL_BYTES_V2];
    assert_eq!(
        core.viewport(&mut buf, CELL_BYTES_V2),
        core.cols() * core.rows()
    );
    buf
}
fn cell(buf: &[u8], cols: u32, row: u32, col: u32) -> &[u8] {
    let i = (row * cols + col) as usize * CELL_BYTES_V2;
    &buf[i..i + CELL_BYTES_V2]
}

#[test]
fn red_bold_text_is_encoded_with_explicit_fg_and_bold() {
    let mut c = Core::new(10, 3, 100, 0xd4d4d4, 0x1e1e1e, 0).unwrap();
    c.write(b"\x1b[1;31mhi\x1b[0m");
    let v = viewport(&mut c);
    let h = cell(&v, 10, 0, 0);
    assert_eq!(&h[0..4], &('h' as u32).to_le_bytes());
    assert_eq!(h[10] & 1, 1); // bold
    assert_eq!(h[12] & 1, 1); // explicit fg
    assert_ne!(&h[4..7], &[0, 0, 0]);
}

#[test]
fn wide_glyph_has_width_two_then_a_zero_width_tail() {
    let mut c = Core::new(10, 3, 100, 0, 0, 0).unwrap();
    c.write("漢".as_bytes());
    let v = viewport(&mut c);
    assert_eq!(cell(&v, 10, 0, 0)[11], 2);
    assert_eq!(cell(&v, 10, 0, 1)[11], 0);
}

#[test]
fn dirty_rows_are_reported_then_cleared() {
    let mut c = Core::new(10, 3, 100, 0, 0, 0).unwrap();
    c.write(b"x");
    c.update();
    assert!(c.is_dirty() >= 1);
    assert!(c.is_dirty_row(0));
    c.clear_dirty();
    assert_eq!(c.is_dirty(), 0);
    assert!(!c.is_dirty_row(0));
}

#[test]
fn cursor_follows_output_and_modes_report() {
    let mut c = Core::new(10, 3, 100, 0, 0, 0).unwrap();
    c.write(b"ab\x1b[?1h\x1b[?2004h\x1b[?1049h");
    c.update();
    let cur = c.cursor();
    assert_eq!((cur.row, cur.col), (0, 2));
    assert!(cur.visible);
    assert!(c.cursor_keys_app());
    assert!(c.bracketed_paste());
    assert!(c.using_alt_screen());
}

#[test]
fn viewport_into_too_small_buffer_writes_nothing() {
    let mut c = Core::new(10, 3, 100, 0, 0, 0).unwrap();
    c.update();
    let mut tiny = [0u8; 4];
    assert_eq!(c.viewport(&mut tiny, CELL_BYTES_V2), 0);
}
