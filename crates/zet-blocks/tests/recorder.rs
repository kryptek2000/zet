use zet_blocks::{Recorder, Store};
use zet_term::Terminal;

fn feed_pump(t: &mut Terminal, rec: &mut Recorder, bytes: &[u8]) {
    t.feed(bytes);
    rec.pump(t, "/tmp");
}

fn recorder() -> (Terminal, Recorder) {
    let store = Store::open_in_memory().unwrap();
    let rec = Recorder::new(store, "s1");
    (Terminal::new(40, 10), rec)
}

#[test]
fn x_markers_feed_vocab_not_blocks() {
    let (mut t, mut rec) = recorder();
    feed_pump(&mut t, &mut rec, b"\x1b]133;X;ll gco\x07");
    assert_eq!(
        rec.drain_cmd_names(),
        vec!["ll".to_string(), "gco".to_string()]
    );
    // Drained once; and no block was recorded.
    assert!(rec.drain_cmd_names().is_empty());
    assert!(rec.blocks().unwrap().is_empty());
}

#[test]
fn records_command_with_output_and_exit() {
    let (mut t, mut rec) = recorder();
    feed_pump(&mut t, &mut rec, b"$ ");
    feed_pump(&mut t, &mut rec, b"\x1b]133;B\x07");
    feed_pump(&mut t, &mut rec, b"echo hi");
    feed_pump(&mut t, &mut rec, b"\x1b]133;C\x07");
    feed_pump(&mut t, &mut rec, b"\r\nhi\r\n");
    feed_pump(&mut t, &mut rec, b"\x1b]133;D;2\x07");
    let blocks = rec.blocks().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].command, "echo hi");
    assert_eq!(blocks[0].exit_code, 2);
    assert_eq!(blocks[0].output, "hi");
    assert_eq!(blocks[0].cwd, "/tmp");
    assert_eq!(blocks[0].session, "s1");
}

#[test]
fn empty_prompt_produces_no_block() {
    let (mut t, mut rec) = recorder();
    feed_pump(&mut t, &mut rec, b"$ ");
    feed_pump(&mut t, &mut rec, b"\x1b]133;B\x07");
    feed_pump(&mut t, &mut rec, b"\x1b]133;C\x07");
    feed_pump(&mut t, &mut rec, b"\r\n");
    feed_pump(&mut t, &mut rec, b"\x1b]133;D;0\x07");
    assert!(rec.blocks().unwrap().is_empty());
}

#[test]
fn scrolled_output_is_captured_whole() {
    let store = Store::open_in_memory().unwrap();
    let mut rec = Recorder::new(store, "s1");
    let mut t = Terminal::new(40, 4);
    feed_pump(&mut t, &mut rec, b"$ ");
    feed_pump(&mut t, &mut rec, b"\x1b]133;B\x07");
    feed_pump(&mut t, &mut rec, b"seq 1 6");
    feed_pump(&mut t, &mut rec, b"\x1b]133;C\x07");
    feed_pump(&mut t, &mut rec, b"\r\n1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n");
    feed_pump(&mut t, &mut rec, b"\x1b]133;D;0\x07");
    let blocks = rec.blocks().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].command, "seq 1 6");
    assert_eq!(blocks[0].output, "1\n2\n3\n4\n5\n6");
}

#[test]
fn two_commands_make_two_blocks() {
    let (mut t, mut rec) = recorder();
    for (cmd, out) in [("echo a", "a"), ("echo b", "b")] {
        feed_pump(&mut t, &mut rec, b"$ ");
        feed_pump(&mut t, &mut rec, b"\x1b]133;B\x07");
        feed_pump(&mut t, &mut rec, cmd.as_bytes());
        feed_pump(&mut t, &mut rec, b"\x1b]133;C\x07");
        let mut bytes = vec![b'\r', b'\n'];
        bytes.extend_from_slice(out.as_bytes());
        bytes.extend_from_slice(b"\r\n");
        feed_pump(&mut t, &mut rec, &bytes);
        feed_pump(&mut t, &mut rec, b"\x1b]133;D;0\x07");
    }
    let blocks = rec.blocks().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].command, "echo a");
    assert_eq!(blocks[1].command, "echo b");
}

#[test]
fn one_feed_one_pump_is_enough() {
    // Markers snapshot positions at parse time: even a single feed with
    // scrolls inside, drained once, records the right block.
    let (mut t, mut rec) = recorder();
    t.feed(b"$ \x1b]133;B\x07echo hi\x1b]133;C\x07\r\nhi\r\n\x1b]133;D;0\x07");
    rec.pump(&mut t, "/tmp");
    let blocks = rec.blocks().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].command, "echo hi");
    assert_eq!(blocks[0].output, "hi");
    assert_eq!(blocks[0].exit_code, 0);
}

#[test]
fn same_row_output_without_newline() {
    // `printf hi`: no trailing newline, D fires on the command row itself.
    // The tail strips the exact prompt+command prefix (no prompt leak).
    let (mut t, mut rec) = recorder();
    t.feed(b"$ \x1b]133;B\x07printf hi\x1b]133;C\x07hi\x1b]133;D;0\x07$ ");
    rec.pump(&mut t, "/tmp");
    let blocks = rec.blocks().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].command, "printf hi");
    assert_eq!(blocks[0].output, "hi");
}

#[test]
fn next_prompt_does_not_leak_into_output() {
    // The prompt printed after D lands on D's row: lazy reads must use the
    // parse-time snapshot, not current grid content.
    let (mut t, mut rec) = recorder();
    t.feed(b"$ \x1b]133;B\x07echo hi\x1b]133;C\x07\r\nhi\r\n\x1b]133;D;0\x07$ next");
    rec.pump(&mut t, "/tmp");
    let blocks = rec.blocks().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].output, "hi");
}

#[test]
fn current_input_tracks_typing_until_exec() {
    let (mut t, mut rec) = recorder();
    assert_eq!(rec.current_input(&t), None);
    feed_pump(&mut t, &mut rec, b"$ ");
    feed_pump(&mut t, &mut rec, b"\x1b]133;B\x07");
    feed_pump(&mut t, &mut rec, b"ec");
    let (text, col, _row) = rec.current_input(&t).expect("typing visible");
    assert_eq!(text, "ec");
    assert_eq!(col, 2);
    feed_pump(&mut t, &mut rec, b"ho");
    assert_eq!(
        rec.current_input(&t).map(|(t, _, _)| t).as_deref(),
        Some("echo")
    );
    // Executing hides the ghost source; done clears it.
    feed_pump(&mut t, &mut rec, b"\r\n\x1b]133;C\x07");
    assert_eq!(rec.current_input(&t), None);
    feed_pump(&mut t, &mut rec, b"echo\r\n\x1b]133;D;0\x07");
    assert_eq!(rec.current_input(&t), None);
}
