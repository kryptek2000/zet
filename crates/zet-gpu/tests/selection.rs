use zet_gpu::selection::{pix_to_cell, Selection};
use zet_term::Terminal;

fn fed() -> Terminal {
    let mut t = Terminal::new(20, 5);
    t.feed(b"hello world\r\nsecond line");
    t
}

#[test]
fn pix_maps_to_cells_and_clamps() {
    // 10x20 cells, no scrollback.
    let c = pix_to_cell(25.0, 45.0, 10, 20, 20, 5, 0);
    assert_eq!((c.col, c.row), (2, 2));
    // Negative clamps to zero, overflow clamps to grid edge.
    let c = pix_to_cell(-5.0, -5.0, 10, 20, 20, 5, 0);
    assert_eq!((c.col, c.row), (0, 0));
    let c = pix_to_cell(500.0, 500.0, 10, 20, 20, 5, 0);
    assert_eq!((c.col, c.row), (19, 4));
    // Rows land in absolute coords (scrollback offset).
    let c = pix_to_cell(5.0, 5.0, 10, 20, 20, 5, 3);
    assert_eq!((c.col, c.row), (0, 3));
}

#[test]
fn empty_click_selects_nothing() {
    let t = fed();
    let s = Selection::new((3, 0), (3, 0));
    assert!(s.is_empty());
    assert!(!s.contains(3, 0));
    assert_eq!(s.text(&t), "");
}

#[test]
fn single_row_slice() {
    let t = fed();
    let s = Selection::new((0, 0), (4, 0));
    assert!(s.contains(0, 0));
    assert!(s.contains(4, 0));
    assert!(!s.contains(5, 0));
    assert!(!s.contains(0, 1));
    // Reversed drag normalizes.
    let r = Selection::new((4, 0), (0, 0));
    assert_eq!(s.text(&t), r.text(&t));
    assert_eq!(s.text(&t), "hello");
}

#[test]
fn multi_row_joins_with_newlines() {
    let t = fed();
    let s = Selection::new((6, 0), (5, 1));
    assert_eq!(s.text(&t), "world\nsecond");
}

#[test]
fn alt_screen_addresses_visible_grid() {
    use zet_term::Terminal;
    let mut t = Terminal::new(20, 5);
    for i in 0..8 {
        t.feed(format!("main line {i}\r\n").as_bytes());
    }
    assert!(t.scrollback_len() > 0);
    t.feed(b"\x1b[?1049h");
    t.feed(b"alt text");
    // Row 0 is the alt grid, NOT main scrollback: offset is 0.
    let s = Selection::new((0, 0), (7, 0));
    assert_eq!(s.text(&t), "alt text");
    t.feed(b"\x1b[?1049l");
    // Back on main: scrollback addressing resumes.
    let s = Selection::new((0, 0), (4, 0));
    let want: String = t.scrollback_line(0).unwrap().chars().take(5).collect();
    assert_eq!(s.text(&t), want.trim_end());
}

#[test]
fn select_all_covers_scrollback_and_grid() {
    let mut t = Terminal::new(20, 3);
    t.feed(b"old line\r\n");
    t.feed(b"hello world\r\nsecond line");
    let total = t.scrollback_len() + 3;
    let s = Selection::all(20, total);
    assert!(!s.is_empty());
    let text = s.text(&t);
    assert!(text.contains("old line"), "{text}");
    assert!(text.contains("hello world"), "{text}");
    assert!(text.contains("second line"), "{text}");
}

#[test]
fn select_all_empty_when_nothing_there() {
    assert!(Selection::all(20, 0).is_empty());
    assert!(Selection::all(0, 5).is_empty());
}

#[test]
fn drag_past_edge_scrolls_view() {
    use zet_gpu::selection::drag_scroll_lines;
    // Mouse above the view (y<0) scrolls up; below scrolls down;
    // inside stays put.
    assert!(drag_scroll_lines(-4.0, 400.0, 20.0) > 0);
    assert!(drag_scroll_lines(404.0, 400.0, 20.0) < 0);
    assert_eq!(drag_scroll_lines(0.0, 400.0, 20.0), 0);
    assert_eq!(drag_scroll_lines(200.0, 400.0, 20.0), 0);
    assert_eq!(drag_scroll_lines(399.0, 400.0, 20.0), 0);
    // Acceleration: further past the edge scrolls faster, capped.
    let near = drag_scroll_lines(-4.0, 400.0, 20.0);
    let far = drag_scroll_lines(-400.0, 400.0, 20.0);
    assert!(far > near, "{near} vs {far}");
    assert!(drag_scroll_lines(-10000.0, 400.0, 20.0) <= 24);
    assert!(drag_scroll_lines(10000.0, 400.0, 20.0) >= -24);
}
