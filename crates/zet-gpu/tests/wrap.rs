use zet_gpu::renderer::wrap_ghost;

#[test]
fn short_text_stays_on_one_row() {
    let cells = wrap_ghost(2, 3, "hi", 10, 5);
    assert_eq!(cells, vec![(3, 2, 'h'), (4, 2, 'i')]);
}

#[test]
fn long_text_wraps_across_rows() {
    // 25 chars from col 5 of a 10-col grid: 5 + 10 + 10.
    let text: String = "a".repeat(25);
    let cells = wrap_ghost(0, 5, &text, 10, 5);
    assert_eq!(cells.len(), 25);
    assert_eq!((cells[0].0, cells[0].1), (5, 0));
    assert_eq!((cells[4].0, cells[4].1), (9, 0));
    assert_eq!((cells[5].0, cells[5].1), (0, 1));
    assert_eq!((cells[24].0, cells[24].1), (9, 2));
}

#[test]
fn wrapping_clips_at_grid_bottom() {
    let text: String = "b".repeat(30);
    let cells = wrap_ghost(3, 8, &text, 10, 5);
    // Row 3 cols 8-9 (2), row 4 cols 0-9 (10): 12 visible, rest cut.
    assert_eq!(cells.len(), 12);
    assert!(cells.iter().all(|(_, y, _)| *y < 5));
}

#[test]
fn explicit_newlines_break_rows() {
    let cells = wrap_ghost(0, 8, "ab\ncd", 10, 5);
    assert_eq!(
        cells,
        vec![(8, 0, 'a'), (9, 0, 'b'), (0, 1, 'c'), (1, 1, 'd')]
    );
}
