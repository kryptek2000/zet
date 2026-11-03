//! Behavioral regression coverage for DEC mouse tracking and SGR negotiation.
use zet_term::Terminal;

#[test]
fn mouse_reporting_starts_disabled() {
    let term = Terminal::new(80, 24);
    assert_eq!(term.mouse_mode(), 0);
    assert!(!term.mouse_sgr());
}

#[test]
fn supported_mouse_tracking_modes_toggle_in_the_terminal_parser() {
    let mut term = Terminal::new(80, 24);
    for mode in [1000, 1002, 1003] {
        term.feed(format!("\x1b[?{mode}h").as_bytes());
        assert_eq!(term.mouse_mode(), mode);
        term.feed(format!("\x1b[?{mode}l").as_bytes());
        assert_eq!(term.mouse_mode(), 0);
    }
}

#[test]
fn earlier_mode_reset_does_not_clear_newer_active_mode() {
    let mut term = Terminal::new(80, 24);
    term.feed(b"\x1b[?1000h\x1b[?1002h\x1b[?1000l");
    assert_eq!(term.mouse_mode(), 1002);
    term.feed(b"\x1b[?1002l");
    assert_eq!(term.mouse_mode(), 0);
}

#[test]
fn sgr_encoding_negotiates_independently_of_tracking() {
    let mut term = Terminal::new(80, 24);
    term.feed(b"\x1b[?1006h");
    assert!(term.mouse_sgr());
    assert_eq!(term.mouse_mode(), 0);
    term.feed(b"\x1b[?1003h");
    assert!(term.mouse_sgr());
    assert_eq!(term.mouse_mode(), 1003);
    term.feed(b"\x1b[?1006l");
    assert!(!term.mouse_sgr());
    assert_eq!(term.mouse_mode(), 1003);
}

#[test]
fn combined_sequences_set_and_clear_both_modes() {
    let mut term = Terminal::new(80, 24);
    term.feed(b"\x1b[?1000;1006h");
    assert_eq!(term.mouse_mode(), 1000);
    assert!(term.mouse_sgr());
    term.feed(b"\x1b[?1000;1006l");
    assert_eq!(term.mouse_mode(), 0);
    assert!(!term.mouse_sgr());
}

#[test]
fn fragmented_escape_sequence_updates_only_on_completion() {
    let mut term = Terminal::new(80, 24);
    term.feed(b"\x1b[?100");
    assert_eq!(term.mouse_mode(), 0);
    term.feed(b"2;1006h");
    assert_eq!(term.mouse_mode(), 1002);
    assert!(term.mouse_sgr());
}

#[test]
fn alternate_screen_switch_does_not_change_mouse_negotiation() {
    let mut term = Terminal::new(80, 24);
    term.feed(b"\x1b[?1002;1006h");
    term.feed(b"\x1b[?1049h");
    assert!(term.alt_active());
    assert_eq!(term.mouse_mode(), 1002);
    assert!(term.mouse_sgr());
    term.feed(b"\x1b[?1049l");
    assert!(!term.alt_active());
    assert_eq!(term.mouse_mode(), 1002);
    assert!(term.mouse_sgr());
}
