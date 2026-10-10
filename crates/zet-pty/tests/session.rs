use zet_pty::Session;

#[test]
fn spawn_echo_returns_output_and_exit_code() {
    let mut s = Session::spawn("sh", &["-c", "echo hello"], 80, 24).unwrap();
    let mut out = Vec::new();
    let code = s.wait_with_output(&mut out).unwrap();
    assert_eq!(code, 0);
    assert!(
        String::from_utf8_lossy(&out).contains("hello"),
        "output was: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn interactive_write_and_nonzero_exit() {
    let mut s = Session::spawn("sh", &[], 80, 24).unwrap();
    s.write_all(b"exit 42\n").unwrap();
    let mut out = Vec::new();
    let code = s.wait_with_output(&mut out).unwrap();
    assert_eq!(code, 42);
    assert_eq!(s.try_exit().unwrap(), Some(42));
}

#[test]
fn pty_output_feeds_term_grid() {
    use zet_term::Terminal;

    let mut s = Session::spawn("sh", &["-c", "printf 'hi\\n'"], 80, 24).unwrap();
    let mut out = Vec::new();
    s.wait_with_output(&mut out).unwrap();
    let mut t = Terminal::new(80, 24);
    t.feed(&out);
    assert_eq!(t.line_text(0).trim_end(), "hi");
}

#[test]
fn cloned_reader_streams_output_from_thread() {
    use std::io::Read;

    let s = Session::spawn("sh", &["-c", "echo from-thread"], 80, 24).unwrap();
    let mut reader = s.clone_reader().unwrap();
    let out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        buf
    })
    .join()
    .unwrap();
    assert!(String::from_utf8_lossy(&out).contains("from-thread"));
}

#[test]
fn consecutive_writes_all_reach_shell() {
    let mut s = Session::spawn("sh", &[], 80, 24).unwrap();
    s.write_all(b"echo ").unwrap();
    s.write_all(b"hi\n").unwrap();
    s.write_all(b"exit\n").unwrap();
    let mut out = Vec::new();
    s.wait_with_output(&mut out).unwrap();
    assert!(
        String::from_utf8_lossy(&out).contains("hi"),
        "output was: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
#[cfg(unix)]
fn erase_char_matches_line_discipline() {
    let s = Session::spawn("sh", &[], 80, 24).unwrap();
    let erase = s.erase_char();
    assert!(
        erase == 0x7f || erase == 0x08,
        "implausible VERASE {erase:#x}"
    );
}
