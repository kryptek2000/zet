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
        // Alt screen shows its own grid: rows address the visible grid.
        let sb = term.abs_base();
        let mut out = Vec::new();
        for row in s.row..=e.row {
            let line: String = if row < sb {
                term.scrollback_line(row).unwrap_or_default()
            } else {
                term.line_text(row - sb)
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
