use zet_term::Terminal;

#[test]
fn hundred_k_lines_feed_without_corruption() {
    let rows = 10usize;
    let cols = 12usize;
    let cap = 100_000usize;
    let mut t = Terminal::with_scrollback(cols, rows, cap);

    let mut input = String::with_capacity(100_000 * 8);
    for i in 0..100_000usize {
        use std::fmt::Write as _;
        let _ = writeln!(input, "line {i}");
    }
    t.feed(input.as_bytes());

    assert_eq!(
        t.scrollback_len() + rows,
        100_001,
        "expected 100000 lines plus the final empty row, got scrollback={} + rows={}",
        t.scrollback_len(),
        rows
    );
    assert_eq!(t.scrollback_line(0).unwrap().trim(), "line 0");
    assert_eq!(t.scrollback_line(1).unwrap().trim(), "line 1");
    assert_eq!(t.scrollback_line(99_989).unwrap().trim(), "line 99989");
    assert_eq!(t.line_text(rows - 2).trim(), "line 99999");
}

#[test]
fn vt_sequences_feed_without_panic() {
    let mut t = Terminal::new(80, 24);
    let seqs: &[&[u8]] = &[
        b"\x1b[2J\x1b[H",
        b"\x1b[31mred\x1b[0m ",
        b"\x1b[1;32;43mbold green on yellow\x1b[0m",
        b"\x1b[?7l\x1b[?7h",
        b"\x1b[?1049h\x1b[?1049l",
        b"\x1b[?2004h\x1b[?2004l",
        b"\x1b[?25l\x1b[?25h",
        b"abc\x08 \x08",
        b"\t\ttabbed\r\n",
        b"\x1b[5;10Hplaced",
        b"\x1b[2A\x1b[3C",
        b"\x1b[1B\x1b[20D",
        b"\x1bM",
        b"\x1bE\x1bD",
        b"\x1b[2;4r\x1b[4;1H\n\x1b[r",
        b"\x1b(B\x1b)B\x1b(0",
        b"a\x1b]133;B\x07cmd\x1b]133;C\x07\r\nout\r\n\x1b]133;D;3\x07",
        b"\x1b]133;X;ll gco\x07",
        b"\xff\xfe!\xc3\xa9",
    ];
    for s in seqs {
        t.feed(s);
    }
    assert!(t.cell(0, 0).is_some());
}

#[test]
fn bench_100k_lines_feed_timing() {
    let rows = 10usize;
    let cols = 12usize;
    let cap = 100_000usize;
    let mut t = zet_term::Terminal::with_scrollback(cols, rows, cap);

    let mut input = String::with_capacity(100_000 * 8);
    for i in 0..cap {
        use std::fmt::Write as _;
        let _ = writeln!(input, "line {i}");
    }
    let start = std::time::Instant::now();
    t.feed(input.as_bytes());
    let elapsed = start.elapsed();
    eprintln!("bench: 100k_lines_feed_ms={}", elapsed.as_millis());
    assert_eq!(t.scrollback_len() + rows, 100_001);
}
