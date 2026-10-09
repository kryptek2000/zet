use zet_term::{Color, Event, Terminal};

#[test]
fn printable_text_advances_cursor_and_stores_cells() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"hello");
    assert_eq!(t.cursor(), (5, 0));
    assert_eq!(t.cell(0, 0).unwrap().c, 'h');
    assert_eq!(t.cell(4, 0).unwrap().c, 'o');
}

#[test]
fn carriage_return_and_linefeed_move_cursor() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"ab\rX");
    assert_eq!(t.cell(0, 0).unwrap().c, 'X');
    assert_eq!(t.cursor(), (1, 0));
    t.feed(b"\nB");
    assert_eq!(t.cell(0, 1).unwrap().c, 'B');
    assert_eq!(t.cursor(), (1, 1));
}

#[test]
fn text_wraps_at_right_margin() {
    let mut t = Terminal::new(5, 24);
    t.feed(b"abcdefg");
    assert_eq!(t.cell(4, 0).unwrap().c, 'e');
    assert_eq!(t.cell(0, 1).unwrap().c, 'f');
    assert_eq!(t.cell(1, 1).unwrap().c, 'g');
    assert_eq!(t.cursor(), (2, 1));
}

#[test]
fn sgr_sets_foreground_color() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[31mR\x1b[0mN");
    assert_eq!(t.cell(0, 0).unwrap().c, 'R');
    assert_eq!(t.cell(0, 0).unwrap().fg, Color::Indexed(1));
    assert_eq!(t.cell(1, 0).unwrap().c, 'N');
    assert_eq!(t.cell(1, 0).unwrap().fg, Color::Default);
}

#[test]
fn sgr_bright_colors() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[91mR\x1b[102mG");
    assert_eq!(t.cell(0, 0).unwrap().fg, Color::Indexed(9));
    assert_eq!(t.cell(1, 0).unwrap().bg, Color::Indexed(10));
}

#[test]
fn cup_moves_cursor() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[3;5HX");
    assert_eq!(t.cell(4, 2).unwrap().c, 'X');
    assert_eq!(t.cursor(), (5, 2));
}

#[test]
fn ed_clears_screen_and_homes_cursor() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"hello\x1b[2J");
    assert_eq!(t.cell(0, 0).unwrap().c, ' ');
    assert_eq!(t.cell(4, 0).unwrap().c, ' ');
    assert_eq!(t.cursor(), (0, 0));
}

#[test]
fn scroll_pushes_lines_into_capped_scrollback() {
    let mut t = Terminal::with_scrollback(5, 2, 2);
    t.feed(b"1\n2\n3\n");
    assert_eq!(t.scrollback_len(), 2);
    assert_eq!(t.scrollback_line(0).unwrap(), "1    ");
    assert_eq!(t.scrollback_line(1).unwrap(), "2    ");
    assert_eq!(t.line_text(0), "3    ");
    t.feed(b"4\n");
    assert_eq!(t.scrollback_len(), 2);
    assert_eq!(t.scrollback_line(0).unwrap(), "2    ");
    assert_eq!(t.scrollback_line(1).unwrap(), "3    ");
}

#[test]
fn el_clears_to_end_of_line() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"hello\r\x1b[2K");
    assert_eq!(t.line_text(0).trim(), "");
    assert_eq!(t.cursor(), (0, 0));
}

#[test]
fn relative_cursor_moves_clamp_at_edges() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[5;10H"); // (9, 4)
    t.feed(b"\x1b[2A\x1b[3C");
    assert_eq!(t.cursor(), (12, 2));
    t.feed(b"\x1b[1B\x1b[20D");
    assert_eq!(t.cursor(), (0, 3));
    t.feed(b"\x1b[99A");
    assert_eq!(t.cursor(), (0, 0));
}

#[test]
fn osc_sequences_are_skipped_silently() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"a\x1b]133;A\x07b");
    assert_eq!(t.cell(0, 0).unwrap().c, 'a');
    assert_eq!(t.cell(1, 0).unwrap().c, 'b');
    assert_eq!(t.cursor(), (2, 0));
}

#[test]
fn osc_events_surface_with_code_and_body() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b]133;A\x07");
    t.feed(b"\x1b]133;D;0\x1b\\");
    assert_eq!(
        t.drain_events(),
        vec![
            Event::Osc {
                code: 133,
                body: "A".to_string()
            },
            Event::Osc {
                code: 133,
                body: "D;0".to_string()
            },
        ]
    );
    assert!(t.drain_events().is_empty());
}

#[test]
fn utf8_multibyte_chars_occupy_one_cell() {
    let mut t = Terminal::new(80, 24);
    t.feed("héllo".as_bytes());
    assert_eq!(t.cell(0, 0).unwrap().c, 'h');
    assert_eq!(t.cell(1, 0).unwrap().c, 'é');
    assert_eq!(t.cell(2, 0).unwrap().c, 'l');
    assert_eq!(t.cursor(), (5, 0));
}

#[test]
fn utf8_split_across_feeds_and_invalid_bytes() {
    let mut t = Terminal::new(80, 24);
    let bytes = "é".as_bytes();
    t.feed(&bytes[..1]);
    assert_eq!(t.cursor(), (0, 0));
    t.feed(&bytes[1..]);
    assert_eq!(t.cell(0, 0).unwrap().c, 'é');
    assert_eq!(t.cursor(), (1, 0));
    t.feed(b"\xff\xfe!");
    assert_eq!(t.cell(1, 0).unwrap().c, '\u{FFFD}');
    assert_eq!(t.cell(2, 0).unwrap().c, '\u{FFFD}');
    assert_eq!(t.cell(3, 0).unwrap().c, '!');
}
