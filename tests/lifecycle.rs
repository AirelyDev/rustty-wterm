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

/// Regression (Task 7, harness/robustness.mjs): on wasm a resize past the
/// address space failed inside the engine and the next `update` aborted.
/// Oversize grids are now refused up front and leave the core untouched.
#[test]
fn oversize_resize_and_init_are_refused() {
    assert!(Core::new(u16::MAX, u16::MAX, 100, 0, 0, 0).is_none());
    let mut c = Core::new(80, 24, 100, 0, 0, 0).unwrap();
    c.write(b"keep");
    c.resize(u16::MAX, u16::MAX);
    c.resize(20000, 20000);
    c.update();
    assert_eq!((c.cols(), c.rows()), (80, 24));
    c.resize(40, 10);
    assert_eq!((c.cols(), c.rows()), (40, 10));
}
