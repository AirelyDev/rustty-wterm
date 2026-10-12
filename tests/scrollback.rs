use rustty_wterm::{cell::CELL_BYTES_V2, Core};

fn fill(core: &mut Core, lines: u32) {
    for i in 0..lines {
        core.write(format!("line {i}\r\n").as_bytes());
    }
}

#[test]
fn scrolled_off_rows_are_counted_and_offset_zero_is_the_newest() {
    let mut c = Core::new(20, 5, 100_000, 0, 0, 0).unwrap();
    fill(&mut c, 12);
    c.update();
    let n = c.scrollback_count();
    assert!(n >= 7, "expected at least 7 rows in history, got {n}");
    let mut row = vec![0u8; 20 * CELL_BYTES_V2];
    assert_eq!(c.scrollback_line(0, &mut row, 20, CELL_BYTES_V2), 20);
    let text: String = (0..6)
        .map(|i| {
            char::from_u32(u32::from_le_bytes(
                row[i * 20..i * 20 + 4].try_into().unwrap(),
            ))
            .unwrap()
        })
        .collect();
    // newest history row is the last one that scrolled off, not "line 0"
    assert_ne!(text, "line 0");
}

#[test]
fn absurd_offsets_return_zero_instead_of_wrapping_to_the_newest_row() {
    let mut c = Core::new(20, 5, 100_000, 0, 0, 0).unwrap();
    fill(&mut c, 12);
    let mut row = vec![0u8; 20 * CELL_BYTES_V2];
    assert_eq!(c.scrollback_line(u32::MAX, &mut row, 20, CELL_BYTES_V2), 0);
    assert_eq!(
        c.scrollback_line(c.scrollback_count(), &mut row, 20, CELL_BYTES_V2),
        0
    );
    assert_eq!(c.grapheme_scrollback(u32::MAX, 0, None), 0);
    assert_eq!(c.scrollback_row_wraps(u32::MAX), -1);
}

#[test]
fn zwj_emoji_reports_full_grapheme_length_then_bytes() {
    let mut c = Core::new(20, 3, 100, 0, 0, 0).unwrap();
    let family = "👨‍👩‍👧";
    c.write(family.as_bytes());
    c.update();
    let need = c.grapheme_viewport(0, 0, None);
    assert_eq!(need as usize, family.len());
    let mut buf = vec![0u8; need as usize];
    assert_eq!(c.grapheme_viewport(0, 0, Some(&mut buf)), need);
    assert_eq!(std::str::from_utf8(&buf).unwrap(), family);
    // too-small buffer still reports the required size and writes nothing
    let mut small = [0u8; 2];
    assert_eq!(c.grapheme_viewport(0, 0, Some(&mut small)), need);
}

#[test]
fn osc8_hyperlink_encodes_uri_and_ids_nul_separated() {
    let mut c = Core::new(30, 3, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b]8;id=abc;https://example.org/x\x1b\\link\x1b]8;;\x1b\\");
    c.update();
    let need = c.hyperlink_viewport(0, 0, None);
    let mut buf = vec![0u8; need as usize];
    c.hyperlink_viewport(0, 0, Some(&mut buf));
    let parts: Vec<&[u8]> = buf.split(|b| *b == 0).collect();
    assert_eq!(parts[0], b"https://example.org/x");
    assert_eq!(parts[1], b"abc");
    // a cell with no link reports 0
    assert_eq!(c.hyperlink_viewport(0, 10, None), 0);
}

#[test]
fn soft_wrapped_rows_report_wrap_flags() {
    let mut c = Core::new(10, 4, 100, 0, 0, 0).unwrap();
    c.write(b"aaaaaaaaaabbbbb"); // 15 chars wrap at col 10
    c.update();
    assert_eq!(c.row_wraps(0) & 1, 1);
    assert_eq!(c.row_wraps(1) & 2, 2);
    assert_eq!(c.row_wraps(99), -1);
}

#[test]
fn implicit_hyperlink_ids_are_decimal_text_and_small_buffers_write_nothing() {
    let mut c = Core::new(30, 3, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b]8;;https://a.example/\x1b\\x\x1b]8;;\x1b\\");
    c.update();
    let need = c.hyperlink_viewport(0, 0, None) as usize;
    let mut buf = vec![0u8; need];
    assert_eq!(c.hyperlink_viewport(0, 0, Some(&mut buf)) as usize, need);
    let parts: Vec<&[u8]> = buf.split(|b| *b == 0).collect();
    assert_eq!(parts[0], b"https://a.example/");
    assert_eq!(parts[1], b"");
    assert!(!parts[2].is_empty() && parts[2].iter().all(u8::is_ascii_digit));
    let mut small = vec![0xAAu8; need - 1];
    assert_eq!(c.hyperlink_viewport(0, 0, Some(&mut small)) as usize, need);
    assert!(small.iter().all(|b| *b == 0xAA));
}

#[test]
fn discarded_rows_count_history_pruned_from_the_oldest_end() {
    let mut c = Core::new(20, 5, 4096, 0, 0, 0).unwrap();
    assert_eq!(c.scrollback_discarded(), 0);
    fill(&mut c, 5000);
    assert!(c.scrollback_count() > 0);
    assert!(c.scrollback_discarded() > 0);
}

/// Every row ever created is on screen, in history, or discarded, whether the
/// lines arrive one write at a time or in a single write (Task 6 harness).
#[test]
fn discarded_rows_account_for_every_pruned_row() {
    for single_write in [false, true] {
        let mut c = Core::new(80, 24, 1000, 0, 0, 0).unwrap();
        let text: String = (0..2000).map(|i| format!("l {i}\r\n")).collect();
        if single_write {
            c.write(text.as_bytes());
        } else {
            fill(&mut c, 2000);
        }
        let disc = c.scrollback_discarded();
        assert!(disc > 0);
        assert_eq!(
            c.scrollback_count() + 24 + disc,
            2001,
            "single_write={single_write}"
        );
    }
}

/// Region scrolls renew page generations without pruning anything (Task 6
/// harness: `scroll-region` reported 144 discarded rows, Zig 0).
#[test]
fn region_scrolls_discard_nothing() {
    let mut c = Core::new(80, 24, 1000, 0, 0, 0).unwrap();
    c.write(b"a\r\nb\x1b[3;6r\x1b[6;1H\x1bDind\x1b[2S\x1b[1T\x1bM");
    assert_eq!(c.scrollback_discarded(), 0);
}

/// ED 3 erases history without discarding (the Zig counter is untouched);
/// RIS resets the page list and with it the counter.
#[test]
fn erase_history_keeps_and_full_reset_clears_the_discarded_count() {
    let mut c = Core::new(80, 24, 1000, 0, 0, 0).unwrap();
    fill(&mut c, 1300);
    let disc = c.scrollback_discarded();
    assert!(disc > 0);
    c.write(b"\x1b[3J");
    assert_eq!(c.scrollback_count(), 0);
    assert_eq!(c.scrollback_discarded(), disc);
    fill(&mut c, 10);
    assert_eq!(c.scrollback_discarded(), disc);
    c.write(b"\x1bc");
    assert_eq!(c.scrollback_discarded(), 0);
}

#[test]
fn history_hyperlinks_and_graphemes_survive_scrolling() {
    let mut c = Core::new(20, 3, 100_000, 0, 0, 0).unwrap();
    c.write("👨‍👩‍👧 \x1b]8;;https://h.example/\x1b\\z\x1b]8;;\x1b\\\r\n".as_bytes());
    fill(&mut c, 10);
    c.update();
    let n = c.scrollback_count();
    assert!(n >= 8);
    let oldest = n - 1;
    assert_eq!(c.grapheme_scrollback(oldest, 0, None) as usize, "👨‍👩‍👧".len());
    assert!(c.hyperlink_scrollback(oldest, 3, None) > 0);
    assert_eq!(c.scrollback_row_wraps(oldest), 0);
}
