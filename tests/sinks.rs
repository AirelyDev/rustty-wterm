use rustty_wterm::sinks::{ClipboardState, DirectoryState, ResponseQueue, TitleState};

#[test]
fn queue_is_fifo_and_drops_the_newest_when_full() {
    let mut q = ResponseQueue::default();
    for i in 0..256u32 {
        q.push(format!("r{i}").as_bytes());
    }
    q.push(b"overflow"); // dropped
    let mut out = [0u8; 64];
    let n = q.pop(&mut out);
    assert_eq!(&out[..n as usize], b"r0");
    for _ in 1..256 {
        assert!(q.pop(&mut out) > 0);
    }
    assert_eq!(q.pop(&mut out), 0); // "overflow" never entered
}

#[test]
fn oversize_response_is_ignored_and_small_output_buffer_keeps_the_entry() {
    let mut q = ResponseQueue::default();
    q.push(&[b'x'; 65]);
    let mut out = [0u8; 64];
    assert_eq!(q.pop(&mut out), 0);
    q.push(b"hello");
    let mut tiny = [0u8; 2];
    assert_eq!(q.pop(&mut tiny), 0);
    assert_eq!(q.pop(&mut out), 5);
}

#[test]
fn title_of_256_bytes_is_ignored_255_is_kept() {
    let mut t = TitleState::default();
    t.set(&vec![b'a'; 256]);
    assert!(!t.changed);
    t.set(&vec![b'a'; 255]);
    assert!(t.changed && t.len == 255);
}

#[test]
fn directory_ignores_invalid_utf8_and_unchanged_values() {
    let mut d = DirectoryState::default();
    d.set(&[0xff, 0xfe]);
    assert!(!d.changed);
    d.set(b"file:///home/user");
    assert!(d.changed);
    d.changed = false;
    d.set(b"file:///home/user");
    assert!(!d.changed);
}

#[test]
fn clipboard_accepts_only_c_selector_valid_base64_utf8() {
    let mut c = ClipboardState::default();
    c.set(b'c', b"?");
    assert!(!c.changed);
    c.set(b'p', b"aGk=");
    assert!(!c.changed);
    c.set(b'c', b"!!notbase64!!");
    assert!(!c.changed);
    c.set(b'c', b"/w==");
    assert!(!c.changed); // 0xff: not UTF-8
    c.set(b'c', b"aGk=");
    assert!(c.changed && &c.bytes[..c.len as usize] == b"hi");
}
