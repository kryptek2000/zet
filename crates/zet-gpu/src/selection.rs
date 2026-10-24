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

/// Tab-bar hit testing with an optional leading rail.
///
/// Clicks in the rail area are rejected (`None`) so a visible rail cannot
/// accidentally activate a tab. Clicks elsewhere use the same segment math
/// as the painted tab bar, keeping clickable and painted regions aligned.
pub fn tabbar_hit_segment(
    x_cell: usize,
    rail_cols: usize,
    cols: usize,
    n_tabs: usize,
) -> Option<usize> {
    if n_tabs == 0 || cols == 0 || x_cell >= cols || x_cell < rail_cols {
        return None;
    }
    Some(tab_segment(x_cell, cols, n_tabs))
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
        assert_eq!(tabbar_hit_segment(4, 4, 10, 3), Some(1));
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
                Some(super::tab_segment(x, 20, 4))
            };
            assert_eq!(got, expected, "x={x}");
        }
    }
}
