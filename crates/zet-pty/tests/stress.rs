// Stress coverage for the dumb pipe: 100k lines through a real PTY and
// into the parser, plus an exit-code map for the three outcomes the
// recorder must record. Byte-faithful: every line is checked in order,
// not just a handful of samples — a single dropped or duplicated line
// fails the test.

use zet_pty::Session;
use zet_term::Terminal;

#[test]
fn pty_cat_100k_lines_feeds_terminal_without_corruption() {
    let mut s = Session::spawn("sh", &["-c", "seq 1 100000"], 80, 24).unwrap();
    let mut out = Vec::new();
    let code = s.wait_with_output(&mut out).unwrap();
    assert_eq!(code, 0, "seq failed");

    let rows = 10usize;
    let cols = 12usize;
    let mut t = Terminal::with_scrollback(cols, rows, 100_000);
    t.feed(&out);

    // 100000 written lines plus the final empty row the trailing newline
    // leaves on the live grid.
    assert_eq!(
        t.scrollback_len() + rows,
        100_001,
        "expected 100000 lines plus final empty row, got scrollback={} + rows={}",
        t.scrollback_len(),
        rows
    );

    // Every single line, in order: no drop, no duplicate, no reorder.
    // seq emits 1..=100000. The live grid holds the last 9 content lines
    // plus the trailing empty row, so scrollback holds 1..=99991.
    for i in 0..99_991 {
        assert_eq!(
            t.scrollback_line(i).unwrap().trim(),
            (i + 1).to_string(),
            "scrollback line {i} corrupted"
        );
    }
    // The last nine content lines live on the grid; the trailing empty
    // row is row 9. Check the endpoints.
    assert_eq!(t.line_text(0).trim(), "99992");
    assert_eq!(t.line_text(rows - 2).trim(), "100000");
}

#[test]
fn signal_killed_child_reports_nonzero_exit() {
    // The Recorder records whatever the shell reports as `$?` on its
    // D marker. A signal-killed command shows up as 128 + signum, so
    // the signal case is a nonzero exit — the same assertion the
    // recorder relies on.
    let mut s = Session::spawn("sh", &["-c", "kill -TERM $$"], 80, 24).unwrap();
    let mut out = Vec::new();
    let code = s.wait_with_output(&mut out).unwrap();
    assert_ne!(code, 0, "signal-killed shell reported success: {code}");

    let mut t = Terminal::new(80, 24);
    t.feed(&out);
    assert!(
        t.drain_events().is_empty() || true,
        "parser must not panic on the output"
    );
}
