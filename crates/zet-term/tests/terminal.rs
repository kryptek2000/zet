use zet_term::{Color, Event, Terminal};

#[test]
fn autowrap_splits_long_lines() {
    let mut t = Terminal::new(10, 5);
    t.feed(b"0123456789ABCDE");
    assert_eq!(t.line_text(0), "0123456789");
    assert_eq!(t.line_text(1).trim_end(), "ABCDE");
    assert_eq!(t.cursor(), (5, 1));
}

#[test]
fn exact_fit_leaves_cursor_on_last_cell_until_next_char() {
    // Pending wrap: the classic VT state. Cursor stays put; apps
    // querying position see the last column, not the next row.
    let mut t = Terminal::new(10, 5);
    t.feed(b"0123456789");
    assert_eq!(t.cursor(), (9, 0));
    assert_eq!(t.line_text(1).trim_end(), "");
    t.feed(b"X");
    assert_eq!(t.cursor(), (1, 1));
    assert_eq!(t.line_text(1).chars().next(), Some('X'));
}

#[test]
fn pending_wrap_plus_linefeed_does_not_skip_a_row() {
    let mut t = Terminal::new(10, 5);
    t.feed(b"0123456789\nABC");
    assert_eq!(t.line_text(0), "0123456789");
    assert_eq!(t.line_text(1).trim_end(), "ABC");
    assert_eq!(t.cursor(), (3, 1));
}

#[test]
fn backspace_after_exact_fit_cancels_pending_wrap() {
    let mut t = Terminal::new(10, 5);
    t.feed(b"0123456789");
    t.feed(b"\x08"); // BS: wrap cancelled, cursor left to col 8
    t.feed(b"Z");
    assert_eq!(t.line_text(0), "01234567Z9");
    assert_eq!(t.cursor(), (9, 0));
}

#[test]
fn decawm_off_overwrites_last_column() {
    let mut t = Terminal::new(10, 5);
    t.feed(b"\x1b[?7l");
    t.feed(b"0123456789ABCDE");
    assert_eq!(t.line_text(0), "012345678E");
    assert_eq!(t.cursor(), (9, 0));
    // And back on wraps again (F overwrote col 9 above; G wraps fresh).
    t.feed(b"\x1b[?7h");
    t.feed(b"FG");
    assert_eq!(t.line_text(0).chars().last(), Some('F'));
    assert_eq!(t.line_text(1).trim_end(), "G");
}

#[test]
fn wrap_scrolls_at_bottom() {
    let mut t = Terminal::new(10, 3);
    t.feed(b"aaaaaaaaaaBBBBBBBBBBCCCCCCCCCCDDDDD");
    assert_eq!(t.scrollback_len(), 1);
    assert_eq!(t.line_text(0), "BBBBBBBBBB");
    assert_eq!(t.line_text(2).trim_end(), "DDDDD");
}

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
fn resize_preserves_overlapping_content() {
    let mut t = Terminal::new(10, 4);
    t.feed(b"hello\nworld\n");
    t.resize(5, 3);
    assert_eq!(t.line_text(0), "hello");
    assert_eq!(t.line_text(1), "world");
    assert_eq!(t.line_text(2), "     ");
    t.feed(b"XY");
    assert_eq!(t.cell(0, 2).unwrap().c, 'X');
    assert_eq!(t.cursor(), (2, 2));
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

#[test]
fn private_csi_sequences_parse_without_leaking() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"a\x1b[?2004h\x1b[?25lX");
    assert_eq!(t.cell(0, 0).unwrap().c, 'a');
    assert_eq!(t.cell(1, 0).unwrap().c, 'X');
    assert_eq!(t.cursor(), (2, 0));
    assert!(!t.cursor_visible());
    t.feed(b"\x1b[?25h");
    assert!(t.cursor_visible());
}

#[test]
fn cursor_save_restore_roundtrip() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[5;10H\x1b7\x1b[1;1HX\x1b8Y");
    assert_eq!(t.cell(0, 0).unwrap().c, 'X');
    assert_eq!(t.cell(9, 4).unwrap().c, 'Y');
    assert_eq!(t.cursor(), (10, 4));
    let mut u = Terminal::new(80, 24);
    u.feed(b"\x1b[3;3H\x1b[s\x1b[9;9H\x1b[u");
    assert_eq!(u.cursor(), (2, 2));
}

#[test]
fn backspace_moves_cursor_back_for_overwrite() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"ab\x08X");
    assert_eq!(t.line_text(0).trim_end(), "aX");
    assert_eq!(t.cursor(), (2, 0));
    let mut edge = Terminal::new(80, 24);
    edge.feed(b"\x08\x08Q");
    assert_eq!(edge.cell(0, 0).unwrap().c, 'Q');
    assert_eq!(edge.cursor(), (1, 0));
}

#[test]
fn tab_advances_to_next_stop() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"a\tb");
    assert_eq!(t.cell(0, 0).unwrap().c, 'a');
    assert_eq!(t.cell(8, 0).unwrap().c, 'b');
    assert_eq!(t.cursor(), (9, 0));
}

#[test]
fn vt_ff_act_as_linefeed_and_bel_ignored() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"a\x0bB\x0cC\x07D");
    assert_eq!(t.cell(0, 0).unwrap().c, 'a');
    assert_eq!(t.cell(0, 1).unwrap().c, 'B');
    assert_eq!(t.cell(0, 2).unwrap().c, 'C');
    assert_eq!(t.cell(1, 2).unwrap().c, 'D');
}

#[test]
fn readline_erase_echo_removes_char_visually() {
    // What bash emits when the user hits Backspace: BS, space, BS.
    let mut t = Terminal::new(80, 24);
    t.feed(b"ab");
    t.feed(b"\x08 \x08");
    assert_eq!(t.line_text(0).trim_end(), "a");
    assert_eq!(t.cursor(), (1, 0));
}

#[test]
fn alt_screen_preserves_main_grid_and_cursor() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"main\x1b[5;10H");
    t.feed(b"\x1b[?1049h");
    assert_eq!(t.line_text(0).trim(), "");
    assert_eq!(t.cursor(), (0, 0));
    t.feed(b"alt-screen");
    assert_eq!(t.line_text(0).trim_end(), "alt-screen");
    t.feed(b"\x1b[?1049l");
    assert_eq!(t.line_text(0).trim_end(), "main");
    assert_eq!(t.cursor(), (9, 4));
}

#[test]
fn alt_scrolling_does_not_pollute_main_scrollback() {
    let mut t = Terminal::with_scrollback(5, 2, 10);
    t.feed(b"m1\nm2\n");
    assert_eq!(t.scrollback_len(), 1);
    t.feed(b"\x1b[?1049h");
    t.feed(b"a\nb\nc\nd\n");
    t.feed(b"\x1b[?1049l");
    assert_eq!(t.scrollback_len(), 1);
    assert_eq!(t.scrollback_line(0).unwrap(), "m1   ");
    assert_eq!(t.line_text(0), "m2   ");
}

#[test]
fn alt_reentrancy_and_1047_1048_variants() {
    let mut t = Terminal::new(10, 4);
    t.feed(b"main\x1b[2;2H");
    // Re-entrant enter is a no-op: content and cursor preserved.
    t.feed(b"\x1b[?1049hA\x1b[?1049hB");
    assert_eq!(t.line_text(0).trim_end(), "AB");
    t.feed(b"\x1b[?1049l");
    assert_eq!(t.line_text(0).trim_end(), "main");
    assert_eq!(t.cursor(), (1, 1));
    // Exit without enter is a no-op.
    t.feed(b"\x1b[?1049l");
    assert_eq!(t.line_text(0).trim_end(), "main");
    // 1047: blank switch without cursor save/restore.
    t.feed(b"\x1b[?1047h");
    assert_eq!(t.line_text(0).trim(), "");
    t.feed(b"X");
    t.feed(b"\x1b[?1047l");
    assert_eq!(t.line_text(0).trim_end(), "main");
    // 1048: cursor save/restore only, grid untouched.
    t.feed(b"\x1b[3;3H\x1b[?1048h\x1b[9;9H\x1b[?1048l");
    assert_eq!(t.cursor(), (2, 2));
    assert_eq!(t.line_text(0).trim_end(), "main");
}

#[test]
fn resize_mid_alt_preserves_both_grids() {
    let mut t = Terminal::new(10, 4);
    t.feed(b"main");
    t.feed(b"\x1b[?1049h");
    t.feed(b"alt");
    t.resize(6, 3);
    assert_eq!(t.line_text(0).trim_end(), "alt");
    t.feed(b"\x1b[?1049l");
    assert_eq!(t.line_text(0).trim_end(), "main");
    assert_eq!(t.line_text(1), "      ");
}

#[test]
fn charset_select_consumed_silently() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"a\x1b(Bb\x1b(0c\x1b)Bd");
    assert_eq!(t.line_text(0).trim_end(), "abcd");
    assert_eq!(t.cursor(), (4, 0));
}

#[test]
fn reverse_index_scrolls_down_at_top() {
    let mut t = Terminal::new(80, 4);
    t.feed(b"one\ntwo\n\x1b[1;1H\x1bM");
    assert_eq!(t.line_text(0).trim(), "");
    assert_eq!(t.line_text(1).trim_end(), "one");
    assert_eq!(t.line_text(2).trim_end(), "two");
    // Mid-screen RI just moves up.
    t.feed(b"\x1b[3;1H\x1bM");
    assert_eq!(t.cursor(), (0, 1));
}

#[test]
fn nel_and_ind_feed_lines() {
    let mut t = Terminal::new(80, 4);
    t.feed(b"ab\x1bE");
    assert_eq!(t.cursor(), (0, 1));
    t.feed(b"cd\x1bD");
    assert_eq!(t.cursor(), (2, 2));
}

#[test]
fn scroll_region_constrains_scrolling() {
    let mut t = Terminal::new(10, 6);
    t.feed(b"A\nB\nC\nD\nE\nF");
    t.feed(b"\x1b[2;4r\x1b[4;1H\n");
    // Region rows 1..=3 scrolled; outside untouched.
    assert_eq!(t.line_text(0).trim_end(), "A");
    assert_eq!(t.line_text(1).trim_end(), "C");
    assert_eq!(t.line_text(2).trim_end(), "D");
    assert_eq!(t.line_text(3).trim(), "");
    assert_eq!(t.line_text(4).trim_end(), "E");
    assert_eq!(t.line_text(5).trim_end(), "F");
    // Reset restores full scrolling.
    t.feed(b"\x1b[r");
    t.feed(b"\x1b[6;1H\n");
    assert_eq!(t.line_text(4).trim_end(), "F");
}

#[test]
fn osc133_markers_snapshot_position_and_exit() {
    use zet_term::{MarkerKind, Terminal};
    let mut t = Terminal::new(40, 10);
    t.feed(b"$ \x1b]133;B\x07cmd\x1b]133;C\x07\r\nout\r\n\x1b]133;D;3\x07");
    let marks = t.drain_markers();
    assert_eq!(marks.len(), 3);
    assert_eq!(marks[0].kind, MarkerKind::Input);
    assert_eq!((marks[0].abs_row, marks[0].col), (0, 2));
    assert_eq!(marks[1].kind, MarkerKind::Exec);
    assert_eq!(marks[2].kind, MarkerKind::Done);
    assert_eq!(marks[2].exit_code, 3);
    // Drained: second call is empty, Osc events still flow separately.
    assert!(t.drain_markers().is_empty());
    assert_eq!(t.drain_events().len(), 3);
}

#[test]
fn osc133_x_carries_shell_command_names() {
    use zet_term::{MarkerKind, Terminal};
    let mut t = Terminal::new(40, 10);
    t.feed(b"\x1b]133;X;ll gco myfunc\x07");
    let marks = t.drain_markers();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].kind, MarkerKind::Cmds);
    assert_eq!(marks[0].line, "ll gco myfunc");
    // Empty payload still marks (window just learns nothing).
    t.feed(b"\x1b]133;X;\x07");
    let marks = t.drain_markers();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].line, "");
}

#[test]
fn osc133_ignored_on_alt_screen() {
    use zet_term::Terminal;
    let mut t = Terminal::new(40, 10);
    t.feed(b"\x1b[?1049h\x1b]133;B\x07\x1b]133;D;0\x07\x1b[?1049l");
    assert!(t.drain_markers().is_empty());
}

#[test]
fn wheel_scrolls_view_into_scrollback() {
    let mut t = Terminal::with_scrollback(5, 2, 10);
    t.feed(b"1\n2\n3\n");
    assert_eq!(t.view_offset(), 0);
    assert_eq!(t.visible_line_text(0), "3    ");
    t.scroll_view_up(1);
    assert_eq!(t.view_offset(), 1);
    // View shifted one line back: top row now shows what was above.
    assert_eq!(t.visible_line_text(1), "3    ");
    assert_eq!(t.visible_line_text(0), "2    ");
    t.scroll_view_down(1);
    assert_eq!(t.view_offset(), 0);
    assert_eq!(t.visible_line_text(0), "3    ");
}

#[test]
fn wheel_clamps_at_history_top_and_live_bottom() {
    let mut t = Terminal::with_scrollback(5, 2, 10);
    t.feed(b"1\n2\n");
    // Only 1 scrollback line: up(99) clamps, down never goes negative.
    t.scroll_view_up(99);
    assert_eq!(t.view_offset(), t.scrollback_len());
    t.scroll_view_down(99);
    assert_eq!(t.view_offset(), 0);
    t.snap_to_bottom();
    assert_eq!(t.view_offset(), 0);
}

#[test]
fn output_keeps_view_stable_while_scrolled() {
    let mut t = Terminal::with_scrollback(5, 2, 10);
    t.feed(b"1\n2\n3\n");
    t.scroll_view_up(1);
    let shown = t.visible_line_text(0);
    t.feed(b"4\n");
    // Same content stays in view (offset grew with history).
    assert_eq!(t.visible_line_text(0), shown);
}

#[test]
fn bracketed_paste_mode_tracks_decset() {
    let mut t = Terminal::new(10, 5);
    assert!(!t.bracketed_paste());
    t.feed(b"\x1b[?2004h");
    assert!(t.bracketed_paste());
    t.feed(b"\x1b[?2004l");
    assert!(!t.bracketed_paste());
}
