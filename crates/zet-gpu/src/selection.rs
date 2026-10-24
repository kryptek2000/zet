// Mouse text selection: pixel math, row ranges, text extraction.
// Absolute rows (scrollback_len + visible y) so selections survive scroll.

use zet_term::Terminal;

/// Grid cell in absolute coordinates (row includes scrollback).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPos {
    pub col: usize,
    pub row: usize,
}

/// Drag selection from anchor to active (either direction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: CellPos,
    pub active: CellPos,
}

impl Selection {
    pub fn new((acol, arow): (usize, usize), (ccol, crow): (usize, usize)) -> Self {
        Selection {
            anchor: CellPos {
                col: acol,
                row: arow,
            },
            active: CellPos {
                col: ccol,
                row: crow,
            },
        }
    }

    /// Click without drag: no selection, renders nothing, copies nothing.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.active
    }

    /// Whole buffer (scrollback + grid): anchor top-left, active at the
    /// last cell. Empty when there is nothing to select.
    pub fn all(cols: usize, abs_rows: usize) -> Self {
        if cols == 0 || abs_rows == 0 {
            return Selection::new((0, 0), (0, 0));
        }
        Selection::new((0, 0), (cols - 1, abs_rows - 1))
    }

    /// Row-major order: (start, end).
    pub fn normalized(&self) -> (CellPos, CellPos) {
        if (self.active.row, self.active.col) < (self.anchor.row, self.anchor.col) {
            (self.active, self.anchor)
        } else {
            (self.anchor, self.active)
        }
    }

    pub fn contains(&self, col: usize, abs_row: usize) -> bool {
        if self.is_empty() {
            return false;
        }
        let (s, e) = self.normalized();
        if abs_row < s.row || abs_row > e.row {
            return false;
        }
        if s.row == e.row {
            return col >= s.col && col <= e.col;
        }
        if abs_row == s.row {
            return col >= s.col;
        }
        if abs_row == e.row {
            return col <= e.col;
        }
        true
    }

    /// Selected text, rows joined with LF, each row trailing-trimmed.
    pub fn text(&self, term: &Terminal) -> String {
        if self.is_empty() {
            return String::new();
        }
        let (s, e) = self.normalized();
        // Wheel-shifted view: rows address the visible grid.
        let sb = term.visible_base();
        let mut out = Vec::new();
        for row in s.row..=e.row {
            let line: String = if row < sb {
                term.scrollback_line(row).unwrap_or_default()
            } else {
                term.visible_line_text(row - sb)
            };
            let chars: Vec<char> = line.chars().collect();
            let (a, b) = if s.row == e.row {
                (s.col, e.col)
            } else if row == s.row {
                (s.col, chars.len())
            } else if row == e.row {
                (0, e.col)
            } else {
                (0, chars.len())
            };
            let from = a.min(chars.len());
            // contains() is end-inclusive; slices are exclusive.
            let to = b.saturating_add(1).min(chars.len()).max(from);
            let part: String = chars[from..to].iter().collect();
            out.push(part.trim_end().to_string());
        }
        out.join("\n").trim_end().to_string()
    }
}

/// Window pixels to absolute cell. Clamps to the grid; rows offset by
/// scrollback so a scrolled view still selects the right lines.
/// Tab-bar hit test: equal segments across the grid width, clamped.
/// Pure geometry — the bar owns row 0 whenever more than one tab
/// exists; clicks there switch instead of selecting.
pub fn tab_segment(x_cell: usize, cols: usize, n_tabs: usize) -> usize {
    if n_tabs == 0 || cols == 0 {
        return 0;
    }
    (x_cell * n_tabs / cols).min(n_tabs - 1)
}

/// Painted tab segments for a tab bar that starts after `rail_cols`.
///
/// With a zero-width rail this is identical to the historical full-window
/// mapping. With a nonzero rail, tab controls occupy only the drawable
/// remainder so every displayed tab can have a reachable hit region when
/// enough columns exist.
pub fn tabbar_segments(cols: usize, rail_cols: usize, n_tabs: usize) -> Vec<(usize, usize)> {
    if n_tabs == 0 || cols <= rail_cols {
        return Vec::new();
    }
    let drawable = cols - rail_cols;
    (0..n_tabs)
        .map(|i| {
            let start = rail_cols + (i * drawable).div_ceil(n_tabs);
            let end = if i + 1 == n_tabs {
                cols
            } else {
                rail_cols + ((i + 1) * drawable).div_ceil(n_tabs)
            };
            (start.min(cols), end.max(start).min(cols))
        })
        .collect()
}

/// Shared tab index for a cell. `None` means outside the window or in the
/// excluded rail region.
pub fn tabbar_segment_for_x(
    x_cell: usize,
    rail_cols: usize,
    cols: usize,
    n_tabs: usize,
) -> Option<usize> {
    if n_tabs == 0 || cols == 0 || x_cell >= cols || x_cell < rail_cols {
        return None;
    }
    let drawable = cols.saturating_sub(rail_cols);
    if drawable == 0 {
        return None;
    }
    Some(((x_cell - rail_cols) * n_tabs / drawable).min(n_tabs - 1))
}

/// Backwards-compatible alias.
pub fn tab_bar_segments(cols: usize, rail_cols: usize, n_tabs: usize) -> Vec<(usize, usize)> {
    tabbar_segments(cols, rail_cols, n_tabs)
}

/// Backwards-compatible alias.
pub fn tabbar_hit_segment(
    x_cell: usize,
    rail_cols: usize,
    cols: usize,
    n_tabs: usize,
) -> Option<usize> {
    tabbar_segment_for_x(x_cell, rail_cols, cols, n_tabs)
}

/// Alias for callers that prefer snake_case words.
pub fn tab_bar_hit_segment(
    x_cell: usize,
    rail_cols: usize,
    cols: usize,
    n_tabs: usize,
) -> Option<usize> {
    tabbar_hit_segment(x_cell, rail_cols, cols, n_tabs)
}

pub fn pix_to_cell(
    x: f64,
    y: f64,
    cell_w: u32,
    cell_h: u32,
    cols: usize,
    rows: usize,
    scrollback: usize,
) -> CellPos {
    let col = (x / cell_w.max(1) as f64).floor() as isize;
    let vis = (y / cell_h.max(1) as f64).floor() as isize;
    CellPos {
        col: col.clamp(0, cols.saturating_sub(1) as isize) as usize,
        row: (vis.clamp(0, rows.saturating_sub(1) as isize) as usize) + scrollback,
    }
}

/// Drag-select autoscroll: holding the mouse past the top/bottom edge
/// of the view walks it so the selection reaches off-screen text.
/// Positive scrolls up (into history), negative back toward live.
/// Accelerates with distance past the edge (2 lines at the edge, up to
/// 24 far out); zero inside the view.
pub fn drag_scroll_lines(mouse_y: f64, view_h: f64, cell_h: f64) -> isize {
    let cell_h = cell_h.max(1.0);
    if mouse_y < 0.0 {
        (2.0 + (-mouse_y / cell_h)).min(24.0) as isize
    } else if mouse_y >= view_h {
        -((2.0 + ((mouse_y - view_h) / cell_h)).min(24.0) as isize)
    } else {
        0
    }
}

#[cfg(test)]
mod tabbar_hit_tests {
    use super::tabbar_hit_segment;

    #[test]
    fn zero_rail_uses_existing_segments() {
        assert_eq!(tabbar_hit_segment(0, 0, 10, 3), Some(0));
        assert_eq!(tabbar_hit_segment(9, 0, 10, 3), Some(2));
    }

    #[test]
    fn rail_clicks_are_rejected() {
        assert_eq!(tabbar_hit_segment(0, 4, 10, 3), None);
        assert_eq!(tabbar_hit_segment(3, 4, 10, 3), None);
        assert_eq!(tabbar_hit_segment(4, 4, 10, 3), Some(0));
    }

    #[test]
    fn invalid_clicks_are_rejected() {
        assert_eq!(tabbar_hit_segment(10, 0, 10, 3), None);
        assert_eq!(tabbar_hit_segment(0, 0, 0, 3), None);
        assert_eq!(tabbar_hit_segment(0, 0, 10, 0), None);
    }

    #[test]
    fn painted_and_clickable_segments_agree() {
        for x in 0..20 {
            let got = tabbar_hit_segment(x, 4, 20, 4);
            let expected = if x < 4 {
                None
            } else {
                super::tabbar_segment_for_x(x, 4, 20, 4)
            };
            assert_eq!(got, expected, "x={x}");
        }
    }
}

#[cfg(test)]
mod tabbar_segment_tests {
    use super::{tabbar_segment_for_x, tabbar_segments};

    #[test]
    fn zero_rail_segments_match_legacy_full_window() {
        let segs = tabbar_segments(10, 0, 3);
        assert_eq!(segs, vec![(0, 4), (4, 7), (7, 10)]);
        for x in 0..10 {
            assert_eq!(
                tabbar_segment_for_x(x, 0, 10, 3),
                Some(super::tab_segment(x, 10, 3))
            );
        }
    }

    #[test]
    fn nonzero_rail_excludes_rail_from_tab_controls() {
        let segs = tabbar_segments(104, 24, 5);
        assert_eq!(segs[0].0, 24);
        assert!(segs.iter().all(|(start, end)| *start >= 24 && *end <= 104));
        for x in 0..24 {
            assert_eq!(tabbar_segment_for_x(x, 24, 104, 5), None);
        }
    }

    #[test]
    fn every_tab_reachable_with_enough_columns() {
        let cols = 125;
        let rail = 24;
        let tabs = 5;
        let mut seen = [false; 5];
        for x in rail..cols {
            if let Some(seg) = tabbar_segment_for_x(x, rail, cols, tabs) {
                seen[seg] = true;
            }
        }
        assert!(seen.iter().all(|v| *v));
    }

    #[test]
    fn five_or_more_tabs_do_not_panic() {
        let segs = tabbar_segments(104, 24, 7);
        assert_eq!(segs.len(), 7);
        for x in 0..104 {
            let _ = tabbar_segment_for_x(x, 24, 104, 7);
        }
    }

    #[test]
    fn narrow_degradation_is_deterministic() {
        assert!(tabbar_segments(20, 24, 3).is_empty());
        for x in 0..20 {
            assert_eq!(tabbar_segment_for_x(x, 24, 20, 3), None);
        }
    }

    #[test]
    fn invalid_and_outside_coordinates_are_rejected() {
        assert_eq!(tabbar_segment_for_x(104, 0, 104, 3), None);
        assert_eq!(tabbar_segment_for_x(usize::MAX, 0, 104, 3), None);
        assert!(tabbar_segments(0, 0, 3).is_empty());
        assert!(tabbar_segments(10, 10, 3).is_empty());
    }

    #[test]
    fn painted_and_clickable_regions_agree() {
        for cols in [0usize, 1, 24, 25, 104, 125] {
            for rail in [0usize, 24] {
                for tabs in [1usize, 2, 5, 9] {
                    let segs = tabbar_segments(cols, rail, tabs);
                    for x in 0..cols {
                        let hit = tabbar_segment_for_x(x, rail, cols, tabs);
                        let painted = segs.iter().position(|(start, end)| x >= *start && x < *end);
                        // Rail pixels are intentionally not painted as tabs.
                        if x < rail {
                            assert_eq!(hit, None);
                        } else if cols > rail {
                            assert_eq!(hit, painted);
                        }
                    }
                }
            }
        }
    }
}
