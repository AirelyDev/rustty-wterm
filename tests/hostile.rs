use rustty_wterm::Core;

#[test]
fn resize_to_zero_or_huge_never_aborts() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.resize(0, 0);
    c.resize(u16::MAX, u16::MAX);
    c.resize(1, 1);
    c.write(b"still alive");
    assert_eq!((c.cols(), c.rows()), (1, 1));
}

#[test]
fn garbage_bytes_never_abort() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    let mut x = 0x2545F4914F6CDD1Du64;
    let mut buf = vec![0u8; 1 << 16];
    for _ in 0..64 {
        for b in buf.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        c.write(&buf);
        c.update();
    }
}

#[test]
fn viewport_read_after_shrink_is_total() {
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"x\r\n".repeat(100).as_slice());
    c.update();
    c.resize(5, 2);
    c.update();
    let mut buf = vec![0u8; 5 * 2 * 20];
    assert_eq!(c.viewport(&mut buf, 20), 10);
}
