use rustty_wterm::Core;

fn reply(c: &mut Core) -> Vec<Vec<u8>> {
    let mut all = vec![];
    loop {
        let mut b = [0u8; 64];
        let n = c.read_response(&mut b);
        if n == 0 {
            break;
        }
        all.push(b[..n as usize].to_vec());
    }
    all
}

#[test]
fn da1_dsr_and_cpr_replies_are_the_exact_zig_bytes() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b[c\x1b[5n\x1b[3;7H\x1b[6n");
    assert_eq!(
        reply(&mut c),
        vec![
            b"\x1b[?1;2c".to_vec(),
            b"\x1b[0n".to_vec(),
            b"\x1b[3;7R".to_vec()
        ]
    );
}

#[test]
fn decrqm_reports_set_reset_and_unknown() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b[?2004h\x1b[?2004$p\x1b[?25l\x1b[?25$p\x1b[?9999$p");
    assert_eq!(
        reply(&mut c),
        vec![
            b"\x1b[?2004;1$y".to_vec(),
            b"\x1b[?25;2$y".to_vec(),
            b"\x1b[?9999;0$y".to_vec()
        ]
    );
}

#[test]
fn osc11_query_answers_with_the_configured_background_and_echoes_terminator() {
    let mut c = Core::new(80, 24, 100, 0xd4d4d4, 0x1e1e1e, 0).unwrap();
    c.write(b"\x1b]11;?\x07\x1b]10;?\x1b\\");
    assert_eq!(
        reply(&mut c),
        vec![
            b"\x1b]11;rgb:1e1e/1e1e/1e1e\x07".to_vec(),
            b"\x1b]10;rgb:d4d4/d4d4/d4d4\x1b\\".to_vec(),
        ]
    );
}

#[test]
fn kitty_keyboard_query_reports_flags() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b[>1u\x1b[?u");
    assert_eq!(reply(&mut c), vec![b"\x1b[?1u".to_vec()]);
}

#[test]
fn queries_split_across_writes_still_answer_once() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b[");
    c.write(b"c");
    assert_eq!(reply(&mut c).len(), 1);
}

// -- Beyond the brief: queries the Zig layer leaves unanswered stay unanswered.

#[test]
fn queries_the_zig_layer_ignores_get_no_reply() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    // DA2, DA3, XTVERSION, DECRQSS (SGR), XTGETTCAP, OSC 4 / 12 / 21 queries,
    // colour-scheme DSR, ENQ, XTWINOPS size, OSC 52 read, OSC 5522 read.
    c.write(b"\x1b[>c\x1b[=c\x1b[>q\x1bP$qm\x1b\\\x1bP+q544e\x1b\\");
    c.write(b"\x1b]4;1;?\x07\x1b]12;?\x07\x1b]21;foreground=?\x07\x1b[?996n\x05\x1b[18t\x1b[14t");
    c.write(b"\x1b]52;c;?\x07\x1b]5522;type=read;dGV4dC9wbGFpbg==\x07");
    assert_eq!(reply(&mut c), Vec::<Vec<u8>>::new());
}

#[test]
fn multi_query_osc_colour_operation_queues_one_entry_per_answer() {
    let mut c = Core::new(80, 24, 100, 0x010203, 0x0a0b0c, 0).unwrap();
    c.write(b"\x1b]10;?;?;?\x07"); // fg, bg, cursor: the cursor is not answered
    assert_eq!(
        reply(&mut c),
        vec![
            b"\x1b]10;rgb:0101/0202/0303\x07".to_vec(),
            b"\x1b]11;rgb:0a0a/0b0b/0c0c\x07".to_vec(),
        ]
    );
}

#[test]
fn ansi_decrqm_has_no_question_mark() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b[4h\x1b[4$p\x1b[20$p");
    assert_eq!(
        reply(&mut c),
        vec![b"\x1b[4;1$y".to_vec(), b"\x1b[20;2$y".to_vec()]
    );
}

#[test]
fn full_queue_drops_the_newest_reply() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    for _ in 0..300 {
        c.write(b"\x1b[5n");
    }
    assert_eq!(reply(&mut c).len(), 256);
}

#[test]
fn osc_query_reflects_host_theme_and_overrides() {
    let mut c = Core::new(80, 24, 100, 0xd4d4d4, 0x1e1e1e, 0).unwrap();
    let mut theme = [0u32; 19];
    theme[1] = 0x102030;
    c.set_theme_colors(&theme);
    c.write(b"\x1b]11;?\x07");
    assert_eq!(
        reply(&mut c),
        vec![b"\x1b]11;rgb:1010/2020/3030\x07".to_vec()]
    );
    assert_eq!(c.color_override(1), -1);
    c.write(b"\x1b]11;rgb:ff/ff/00\x07");
    c.set_theme_colors(&theme); // theme changes keep the application override
    assert_eq!(c.color_override(1), 0xffff00);
}

#[test]
fn cpr_is_absolute_even_under_origin_mode() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"\x1b[5;20r\x1b[?6h\x1b[2;3H\x1b[6n");
    assert_eq!(reply(&mut c), vec![b"\x1b[6;3R".to_vec()]);
}
