use rustty_wterm::Core;

#[test]
fn title_is_consume_on_read() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b]2;hello\x07");
    assert_eq!(c.title().map(|t| t.to_vec()), Some(b"hello".to_vec()));
    assert!(c.title().is_none());
}

#[test]
fn bell_counts_and_clears_and_osc_terminating_bel_does_not_count() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x07\x07\x1b]2;t\x07");
    assert_eq!(c.bell_count(), 2);
    assert_eq!(c.bell_count(), 0);
}

#[test]
fn synchronized_output_generation_bumps_only_on_off_to_on() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    let g0 = c.synchronized_output_generation();
    c.write(b"\x1b[?2026h\x1b[?2026h");
    assert_eq!(c.synchronized_output_generation(), g0 + 1);
    c.write(b"\x1b[?2026l\x1b[?2026h");
    assert_eq!(c.synchronized_output_generation(), g0 + 2);
}

#[test]
fn shell_phase_follows_osc133_and_exit_code_is_nan_when_unset() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    assert_eq!(c.shell_phase(), -1);
    c.write(b"\x1b]133;A\x07");
    assert_eq!(c.shell_phase(), 1); // prompt
    c.write(b"\x1b]133;B\x07");
    assert_eq!(c.shell_phase(), 2); // input
    c.write(b"\x1b]133;C\x07");
    assert_eq!(c.shell_phase(), 3); // running
    c.write(b"\x1b]133;D;7\x07");
    assert_eq!(c.shell_phase(), 4); // complete
    assert_eq!(c.shell_exit_code(), 7.0);
}

#[test]
fn osc_colour_override_set_then_reset() {
    let mut c = Core::new(80, 24, 100, 0xd4d4d4, 0x1e1e1e, 0).unwrap();
    assert_eq!(c.color_override(1), -1);
    c.write(b"\x1b]11;rgb:ff/00/00\x07");
    assert_eq!(c.color_override(1), 0xff0000);
    c.write(b"\x1b]111\x07");
    assert_eq!(c.color_override(1), -1);
}

#[test]
fn tracked_position_follows_scroll_and_dies_on_screen_switch() {
    let mut c = Core::new(10, 3, 1000, 0, 0, 0).unwrap();
    c.write(b"abc");
    let id = c.track_position(0, 1);
    assert_ne!(id, 0);
    assert_eq!(c.resolve_position(id), 0.0 * 65536.0 + 1.0);
    c.write(b"\r\n\r\n\r\n\r\n"); // scroll: row 0 stays row 0 in screen coords
    assert_eq!(c.resolve_position(id), 1.0);
    c.write(b"\x1b[?1049h");
    assert_eq!(c.resolve_position(id), -1.0);
    c.release_position(id);
    assert_eq!(c.track_position(99_999, 0), 0); // out of range
}

#[test]
fn position_table_holds_64_then_refuses() {
    let mut c = Core::new(10, 3, 1000, 0, 0, 0).unwrap();
    let ids: Vec<u32> = (0..64).map(|_| c.track_position(0, 0)).collect();
    assert!(ids.iter().all(|i| *i != 0));
    assert_eq!(c.track_position(0, 0), 0);
    c.release_position(ids[0]);
    let again = c.track_position(0, 0);
    assert!(again != 0 && !ids.contains(&again)); // ids never alias
}

// -- Beyond the brief -------------------------------------------------------

#[test]
fn synchronized_output_generation_counts_xtrestore() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b[?2026h\x1b[?2026s\x1b[?2026l"); // +1, save "on", off
    let g = c.synchronized_output_generation();
    c.write(b"\x1b[?2026r"); // restore: off -> on
    assert_eq!(c.synchronized_output_generation(), g + 1);
}

#[test]
fn working_directory_and_clipboard_through_the_stream() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    assert!(c.working_directory().is_none());
    c.write(b"\x1b]7;file:///home/user\x07");
    assert_eq!(c.working_directory(), Some(&b"file:///home/user"[..]));
    c.write(b"\x1b]7;file:///home/user\x07");
    assert!(c.working_directory().is_none()); // identical value: unchanged

    c.write(b"\x1b]52;p;aGk=\x07\x1b]52;c;!!\x07");
    assert!(c.clipboard_write().is_none());
    c.write(b"\x1b]52;c;aGk=\x07\x1b]52;c;/w==\x07"); // second is not UTF-8
    assert_eq!(c.clipboard_write(), Some(&b"hi"[..]));
    assert!(c.clipboard_write().is_none());
}

#[test]
fn long_titles_are_ignored() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    let mut seq = b"\x1b]2;".to_vec();
    seq.extend(std::iter::repeat_n(b'a', 300));
    seq.push(7);
    c.write(&seq);
    assert!(c.title().is_none());
    c.write(b"\x1b]2;\x07");
    assert_eq!(c.title(), Some(&b""[..]));
}

#[test]
fn shell_markers_are_primary_screen_only_and_full_reset_reports_phase_zero() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b[?1049h\x1b]133;A\x07");
    assert_eq!(c.shell_phase(), -1);
    c.write(b"\x1b[?1049l\x1b]133;D;1\x07");
    assert_eq!(c.shell_phase(), 4);
    c.write(b"\x1b]7;file:///x\x07");
    let _ = c.working_directory();
    c.write(b"\x1bc");
    assert_eq!(c.shell_phase(), 0);
    assert!(c.shell_exit_code().is_nan());
    assert_eq!(c.working_directory(), Some(&b""[..]));
}

#[test]
fn positions_on_a_freed_alt_screen_are_dropped_safely() {
    let mut c = Core::new(10, 3, 1000, 0, 0, 0).unwrap();
    c.write(b"\x1b[?1049h");
    let alt = c.track_position(1, 1);
    assert_eq!(c.resolve_position(alt), 65537.0);
    c.write(b"\x1bc"); // frees the alternate screen
    assert_eq!(c.resolve_position(alt), -1.0);
    c.release_position(alt); // already gone: no-op
    c.write(b"\x1b[?1049h");
    let again = c.track_position(0, 0);
    assert!(again > alt);
    c.write(b"\x1b[?1049l");
    assert_eq!(c.resolve_position(again), -1.0);
}

#[test]
fn find_prompt_walks_osc133_prompts_on_the_primary_screen() {
    let mut c = Core::new(20, 10, 10_000, 0, 0, 0).unwrap();
    c.write(b"\x1b]133;A\x07$ ls\r\nout1\r\nout2\r\n\x1b]133;A\x07$ \r\nmore");
    assert_eq!(c.find_prompt(3, -1), 0.0);
    assert_eq!(c.find_prompt(0, 1), 3.0);
    assert_eq!(c.find_prompt(0, -1), -1.0);
    assert_eq!(c.find_prompt(4, 1), -1.0);
    assert_eq!(c.find_prompt(3, 0), -1.0);
    assert_eq!(c.find_prompt(u32::MAX, -1), -1.0);
    c.write(b"\x1b[?1049h");
    assert_eq!(c.find_prompt(3, -1), -1.0);
}
