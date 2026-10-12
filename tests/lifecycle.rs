use rustty_wterm::Core;

#[test]
fn new_core_reports_requested_grid() {
    let core = Core::new(80, 24, 10_000, 0xd4d4d4, 0x1e1e1e, 32 * 1024 * 1024).expect("core");
    assert_eq!((core.cols(), core.rows()), (80, 24));
}

#[test]
fn write_then_resize_keeps_core_usable() {
    let mut core = Core::new(80, 24, 1_000, 0xffffff, 0x000000, 0).unwrap();
    core.write(b"hello\r\nworld");
    core.resize(100, 30);
    assert_eq!((core.cols(), core.rows()), (100, 30));
}

#[test]
fn zero_sized_grid_is_rejected_not_aborted() {
    // wasm panics abort the module; init must return None instead.
    assert!(Core::new(0, 0, 0, 0, 0, 0).is_none());
}
