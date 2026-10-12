use rustty_wterm::cell::{encode_cell, CellFields, CELL_BYTES, CELL_BYTES_V2};

fn base() -> CellFields {
    CellFields {
        codepoint: 'A' as u32,
        fg: None,
        bg: None,
        flags: 0,
        width: 1,
        has_grapheme: false,
        has_hyperlink: false,
        spacer_head: false,
        underline: 0,
        underline_rgb: None,
    }
}

#[test]
fn v1_layout_matches_the_documented_byte_offsets() {
    let mut f = base();
    f.fg = Some([1, 2, 3]);
    f.bg = Some([4, 5, 6]);
    f.flags = 0x01 | 0x20;
    f.width = 2;
    f.has_grapheme = true;
    f.has_hyperlink = true;
    let mut out = [0xAAu8; CELL_BYTES];
    encode_cell(&f, &mut out);
    assert_eq!(&out[0..4], &65u32.to_le_bytes());
    assert_eq!(&out[4..10], &[1, 2, 3, 4, 5, 6]);
    assert_eq!(out[10], 0x21); // bold | inverse
    assert_eq!(out[11], 2); // width
    assert_eq!(out[12], 0b11); // explicit fg | explicit bg
    assert_eq!(out[13], 0b011); // grapheme | hyperlink
    assert_eq!(&out[14..16], &[0, 0]);
}

#[test]
fn spacer_head_sets_content_flag_bit_two_and_keeps_width_one() {
    let mut f = base();
    f.width = 1;
    f.spacer_head = true;
    let mut out = [0u8; CELL_BYTES];
    encode_cell(&f, &mut out);
    assert_eq!(out[11], 1);
    assert_eq!(out[13] & 4, 4);
}

#[test]
fn v2_adds_underline_style_and_colour_and_color_flag_bit_two() {
    let mut f = base();
    f.underline = 3; // curly
    f.underline_rgb = Some([9, 8, 7]);
    let mut out = [0u8; CELL_BYTES_V2];
    encode_cell(&f, &mut out);
    assert_eq!(out[12] & 4, 4);
    assert_eq!(out[16], 3);
    assert_eq!(&out[17..20], &[9, 8, 7]);
}

#[test]
fn v2_without_underline_colour_leaves_bit_two_clear() {
    let mut out = [0u8; CELL_BYTES_V2];
    encode_cell(&base(), &mut out);
    assert_eq!(out[12] & 4, 0);
    assert_eq!(&out[16..20], &[0, 0, 0, 0]);
}
