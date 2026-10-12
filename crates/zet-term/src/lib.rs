use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
}

impl Cell {
    fn blank() -> Self {
        Cell {
            c: ' ',
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pen {
    fg: Color,
    bg: Color,
    bold: bool,
}

impl Pen {
    fn reset(&mut self) {
        *self = Pen {
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
        };
    }

    fn sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.reset();
            return;
        }
        for &p in params {
            match p {
                0 => self.reset(),
                1 => self.bold = true,
                30..=37 => self.fg = Color::Indexed((p - 30) as u8),
                40..=47 => self.bg = Color::Indexed((p - 40) as u8),
                90..=97 => self.fg = Color::Indexed((p - 90 + 8) as u8),
                100..=107 => self.bg = Color::Indexed((p - 100 + 8) as u8),
                _ => {}
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Osc { code: u16, body: String },
}

/// Shell-integration marker (OSC 133, FinalTerm-style), snapshotted with
/// grid position AT PARSE TIME so consumers can drain lazily without
/// positions going stale across scrolls. Only recorded on the main grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub kind: MarkerKind,
    pub abs_row: usize,
    pub col: usize,
    pub exit_code: i32,
    /// Grid row text at parse time (Exec/Done only). The Done row later
    /// gains the next prompt, so lazy reads would leak it into output;
    /// the snapshot predates the prompt. The Exec snapshot lets same-row
    /// output strip the exact prompt+command prefix (wide-char safe).
    pub line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// B: command input starts (cursor = prompt end).
    Input,
    /// C: pre-exec (command runs next).
    Exec,
    /// D: finished; exit_code set.
    Done,
    /// X: shell-reported command names (aliases/functions); the payload
    /// (space-separated) rides in `line`.
    Cmds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Csi {
        current: u16,
        params: [u16; 16],
        count: usize,
        private: Option<u8>,
    },
    Osc {
        saw_esc: bool,
    },
    /// ESC ( X / ESC ) X: charset select; consumes the final byte.
    Scs,
}

pub struct Terminal {
    cols: usize,
    rows: usize,
    grid: Vec<Vec<Cell>>,
    scrollback: VecDeque<Vec<Cell>>,
    scrollback_cap: usize,
    /// Wheel position: 0 = live bottom, N = N lines back into history.
    /// Typing snaps to 0; new output keeps the viewed content stable.
    view_offset: usize,
    cx: usize,
    cy: usize,
    cursor_visible: bool,
    saved_cursor: (usize, usize),
    /// DECAWM (?7): wrap long lines. Pending wrap: the cursor sits on
    /// the last column after an exact-fit line until the next input
    /// resolves it (printable wraps, movement cancels).
    autowrap: bool,
    pending_wrap: bool,
    alt_grid: Vec<Vec<Cell>>,
    alt_active: bool,
    scroll_top: usize,
    scroll_bottom: usize,
    /// DECSET 2004: the hosted app (bash/readline) wraps pastes in
    /// 200~/201~ so newlines insert instead of executing. The window
    /// must honor it on every text-insertion path.
    bracketed_paste: bool,
    pen: Pen,
    state: State,
    osc_buf: Vec<u8>,
    events: Vec<Event>,
    markers: Vec<Marker>,
    utf8_buf: [u8; 4],
    utf8_need: u8,
    utf8_have: u8,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback(cols, rows, 10_000)
    }

    pub fn with_scrollback(cols: usize, rows: usize, scrollback_cap: usize) -> Self {
        Terminal {
            cols,
            rows,
            grid: vec![vec![Cell::blank(); cols]; rows],
            scrollback: VecDeque::new(),
            scrollback_cap,
            view_offset: 0,
            cx: 0,
            cy: 0,
            cursor_visible: true,
            saved_cursor: (0, 0),
            autowrap: true,
            pending_wrap: false,
            alt_grid: Vec::new(),
            alt_active: false,
            scroll_top: 0,
            scroll_bottom: rows - 1,
            bracketed_paste: false,
            pen: Pen {
                fg: Color::Default,
                bg: Color::Default,
                bold: false,
            },
            state: State::Ground,
            osc_buf: Vec::new(),
            events: Vec::new(),
            markers: Vec::new(),
            utf8_buf: [0; 4],
            utf8_need: 0,
            utf8_have: 0,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match self.state {
                State::Ground => self.ground(b),
                State::Escape => self.escape(b),
                State::Scs => self.state = State::Ground,
                State::Osc { saw_esc } => self.osc(b, saw_esc),
                State::Csi {
                    mut current,
                    mut params,
                    mut count,
                    mut private,
                } => {
                    if b.is_ascii_digit() {
                        current = current.saturating_mul(10).saturating_add((b - b'0') as u16);
                        self.state = State::Csi {
                            current,
                            params,
                            count,
                            private,
                        };
                    } else if b == b';' {
                        if count < params.len() {
                            params[count] = current;
                            count += 1;
                        }
                        self.state = State::Csi {
                            current: 0,
                            params,
                            count,
                            private,
                        };
                    } else if (0x3c..=0x3f).contains(&b) && count == 0 && current == 0 {
                        // Private markers (? <= >) and intermediates.
                        if b == b'?' {
                            private = Some(b'?');
                        }
                        self.state = State::Csi {
                            current,
                            params,
                            count,
                            private,
                        };
                    } else if (0x40..=0x7e).contains(&b) {
                        if count < params.len() {
                            params[count] = current;
                            count += 1;
                        }
                        self.dispatch_csi(b, &params[..count], private);
                        self.state = State::Ground;
                    } else {
                        self.state = State::Ground;
                    }
                }
            }
        }
    }

    fn ground(&mut self, b: u8) {
        if b == b'\n' {
            self.pending_wrap = false;
            self.linefeed();
            self.cx = 0;
        } else if b == b'\r' {
            self.pending_wrap = false;
            self.cx = 0;
        } else if b == 0x1b {
            self.state = State::Escape;
        } else if b == 0x08 {
            self.pending_wrap = false;
            self.cx = self.cx.saturating_sub(1);
        } else if b == b'\t' {
            self.pending_wrap = false;
            self.cx = ((self.cx / 8) + 1) * 8;
            if self.cx >= self.cols {
                self.cx = 0;
                self.linefeed();
            }
        } else if b == 0x0b || b == 0x0c {
            self.pending_wrap = false;
            self.linefeed();
            self.cx = 0;
        } else {
            self.text(b);
        }
    }

    fn text(&mut self, b: u8) {
        if self.utf8_need > 0 {
            if (0x80..0xc0).contains(&b) {
                self.utf8_buf[self.utf8_have as usize] = b;
                self.utf8_have += 1;
                if self.utf8_have >= self.utf8_need {
                    let bytes = &self.utf8_buf[..self.utf8_need as usize];
                    let ch = std::str::from_utf8(bytes)
                        .ok()
                        .and_then(|s| s.chars().next())
                        .unwrap_or('\u{FFFD}');
                    self.utf8_need = 0;
                    self.utf8_have = 0;
                    self.put(ch);
                }
                return;
            }
            self.utf8_need = 0;
            self.utf8_have = 0;
        }
        if b < 0x80 {
            if b >= 0x20 {
                self.put(b as char);
            }
        } else if b < 0xc0 {
            self.put('\u{FFFD}');
        } else {
            let need = if b < 0xe0 {
                2
            } else if b < 0xf0 {
                3
            } else if b < 0xf8 {
                4
            } else {
                0
            };
            if need == 0 {
                self.put('\u{FFFD}');
            } else {
                self.utf8_buf[0] = b;
                self.utf8_have = 1;
                self.utf8_need = need;
            }
        }
    }

    fn escape(&mut self, b: u8) {
        if b == b'[' {
            self.state = State::Csi {
                current: 0,
                params: [0; 16],
                count: 0,
                private: None,
            };
        } else if b == b']' {
            self.osc_buf.clear();
            self.state = State::Osc { saw_esc: false };
        } else if b == b'(' || b == b')' || b == b'*' || b == b'+' {
            self.state = State::Scs;
        } else if b == b'7' {
            self.saved_cursor = (self.cx, self.cy);
            self.state = State::Ground;
        } else if b == b'M' {
            self.reverse_index();
            self.state = State::Ground;
        } else if b == b'E' {
            self.linefeed();
            self.cx = 0;
            self.state = State::Ground;
        } else if b == b'D' {
            self.linefeed();
            self.state = State::Ground;
        } else if b == b'8' {
            (self.cx, self.cy) = self.saved_cursor;
            self.cx = self.cx.min(self.cols - 1);
            self.cy = self.cy.min(self.rows - 1);
            self.state = State::Ground;
        } else {
            self.state = State::Ground;
        }
    }

    fn osc(&mut self, b: u8, saw_esc: bool) {
        if b == 0x07 && !saw_esc {
            self.dispatch_osc();
            self.state = State::Ground;
        } else if b == 0x1b {
            self.state = State::Osc { saw_esc: true };
        } else if b == b'\\' && saw_esc {
            self.dispatch_osc();
            self.state = State::Ground;
        } else {
            if saw_esc {
                self.osc_buf.push(0x1b);
            }
            self.osc_buf.push(b);
            self.state = State::Osc { saw_esc: false };
        }
    }

    fn dispatch_osc(&mut self) {
        let text = String::from_utf8_lossy(&self.osc_buf);
        let (code, body) = match text.split_once(';') {
            Some((c, rest)) => (c.parse::<u16>().unwrap_or(0), rest.to_string()),
            None => (0, text.into_owned()),
        };
        self.events.push(Event::Osc {
            code,
            body: body.clone(),
        });
        self.record_marker(code, &body);
        self.osc_buf.clear();
    }

    /// Snapshot a 133 marker with exact grid position, main grid only.
    fn record_marker(&mut self, code: u16, body: &str) {
        if code != 133 || self.alt_active {
            return;
        }
        let kind = match body.chars().next() {
            Some('B') => MarkerKind::Input,
            Some('C') => MarkerKind::Exec,
            Some('D') => MarkerKind::Done,
            Some('X') => MarkerKind::Cmds,
            _ => return,
        };
        let exit_code = if kind == MarkerKind::Done {
            body.split(';')
                .nth(1)
                .unwrap_or("0")
                .parse::<i32>()
                .unwrap_or(0)
        } else {
            0
        };
        let line = match kind {
            MarkerKind::Exec | MarkerKind::Done => self.line_text(self.cy),
            // X payload is `X;<space-separated names>`.
            MarkerKind::Cmds => body.split(';').nth(1).unwrap_or("").to_string(),
            _ => String::new(),
        };
        self.markers.push(Marker {
            kind,
            abs_row: self.scrollback.len() + self.cy,
            col: self.cx,
            exit_code,
            line,
        });
    }

    fn dispatch_csi(&mut self, final_byte: u8, params: &[u16], private: Option<u8>) {
        // Cursor-moving finals cancel a pending wrap (VT behavior).
        match final_byte {
            b'H' | b'f' | b'A' | b'B' | b'C' | b'D' | b'E' | b'M' | b's' | b'u' | b'J' | b'K'
            | b'L' => self.pending_wrap = false,
            _ => {}
        }
        match final_byte {
            b'm' => self.pen.sgr(params),
            b'h' | b'l' if private == Some(b'?') => {
                let show = final_byte == b'h';
                for &p in params {
                    if p == 25 {
                        self.cursor_visible = show;
                    }
                    if p == 7 {
                        self.autowrap = show;
                    }
                    if p == 2004 {
                        self.bracketed_paste = show;
                    }
                }
                for &p in params {
                    match (p, show) {
                        (1049, true) => self.enter_alt(true),
                        (1049, false) => self.exit_alt(true),
                        (1047, true) => self.enter_alt(false),
                        (1047, false) => self.exit_alt(false),
                        (1048, true) => {
                            self.saved_cursor = (self.cx, self.cy);
                        }
                        (1048, false) => {
                            (self.cx, self.cy) = self.saved_cursor;
                            self.cx = self.cx.min(self.cols - 1);
                            self.cy = self.cy.min(self.rows - 1);
                        }
                        _ => {}
                    }
                }
            }
            b's' => {
                self.saved_cursor = (self.cx, self.cy);
            }
            b'u' => {
                (self.cx, self.cy) = self.saved_cursor;
                self.cx = self.cx.min(self.cols - 1);
                self.cy = self.cy.min(self.rows - 1);
            }
            b'r' => {
                let top = params.first().copied().unwrap_or(1).max(1) as usize;
                let bottom = params.get(1).copied().unwrap_or(self.rows as u16) as usize;
                let top = (top - 1).min(self.rows - 1);
                let bottom = bottom.clamp(1, self.rows) - 1;
                if top < bottom {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                } else {
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows - 1;
                }
                self.cx = 0;
                self.cy = self.scroll_top;
            }
            b'H' | b'f' => {
                let row = params.first().copied().unwrap_or(1).max(1);
                let col = params.get(1).copied().unwrap_or(1).max(1);
                self.cy = ((row - 1) as usize).min(self.rows - 1);
                self.cx = ((col - 1) as usize).min(self.cols - 1);
            }
            b'A' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cy = self.cy.saturating_sub(n);
            }
            b'B' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cy = (self.cy + n).min(self.rows - 1);
            }
            b'C' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cx = (self.cx + n).min(self.cols - 1);
            }
            b'D' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cx = self.cx.saturating_sub(n);
            }
            b'J' => {
                if params.first().copied().unwrap_or(0) == 2 {
                    self.grid = vec![vec![Cell::blank(); self.cols]; self.rows];
                    self.cx = 0;
                    self.cy = 0;
                }
            }
            b'K' => {
                let row = &mut self.grid[self.cy];
                match params.first().copied().unwrap_or(0) {
                    1 => row[..=self.cx.min(self.cols - 1)].fill(Cell::blank()),
                    2 => row.fill(Cell::blank()),
                    _ => row[self.cx.min(self.cols)..].fill(Cell::blank()),
                }
            }
            _ => {}
        }
    }

    fn put(&mut self, c: char) {
        // Pending wrap resolves on the next printable: down a row first.
        if self.pending_wrap {
            self.pending_wrap = false;
            self.cx = 0;
            self.linefeed();
        }
        self.grid[self.cy][self.cx] = Cell {
            c,
            fg: self.pen.fg,
            bg: self.pen.bg,
            bold: self.pen.bold,
        };
        self.cx += 1;
        if self.cx >= self.cols {
            // Last column: with autowrap the wrap waits for the next
            // printable (cursor stays put); without it, later chars
            // overwrite the last cell.
            self.cx = self.cols - 1;
            if self.autowrap {
                self.pending_wrap = true;
            }
        }
    }

    fn linefeed(&mut self) {
        if self.cy == self.scroll_bottom {
            if self.scroll_top == 0 && self.scroll_bottom + 1 >= self.rows {
                self.scroll_up();
            } else {
                self.scroll_region_up();
            }
        } else if self.cy + 1 >= self.rows {
            self.cy = self.rows - 1;
        } else {
            self.cy += 1;
        }
    }

    fn scroll_region_up(&mut self) {
        for y in self.scroll_top..self.scroll_bottom {
            let next = self.grid[y + 1].clone();
            self.grid[y] = next;
        }
        self.grid[self.scroll_bottom] = vec![Cell::blank(); self.cols];
    }

    fn reverse_index(&mut self) {
        if self.cy == self.scroll_top {
            let blank = vec![Cell::blank(); self.cols];
            self.grid.remove(self.scroll_bottom);
            self.grid.insert(self.scroll_top, blank);
        } else {
            self.cy = self.cy.saturating_sub(1);
        }
    }

    fn scroll_up(&mut self) {
        let top = self.grid[0].clone();
        self.grid.remove(0);
        self.grid.push(vec![Cell::blank(); self.cols]);
        if self.scrollback_cap > 0 && !self.alt_active {
            self.scrollback.push_back(top);
            while self.scrollback.len() > self.scrollback_cap {
                self.scrollback.pop_front();
            }
            // View stays on the same content while scrolled (the history
            // grew under it); clamped since the cap may eat the top.
            if self.view_offset > 0 {
                self.view_offset = (self.view_offset + 1).min(self.scrollback.len());
            }
        }
    }

    fn blank_grid(&self) -> Vec<Vec<Cell>> {
        vec![vec![Cell::blank(); self.cols]; self.rows]
    }

    /// DECSET 1049/1047: switch to the alternate buffer (blank, no scrollback).
    fn enter_alt(&mut self, save_cursor: bool) {
        if self.alt_active {
            return;
        }
        if save_cursor {
            self.saved_cursor = (self.cx, self.cy);
        }
        let blank = self.blank_grid();
        self.alt_grid = std::mem::replace(&mut self.grid, blank);
        self.cx = 0;
        self.cy = 0;
        self.alt_active = true;
    }

    /// DECRST 1049/1047: restore the main buffer (and cursor for 1049).
    fn exit_alt(&mut self, restore_cursor: bool) {
        if !self.alt_active {
            return;
        }
        self.grid = std::mem::take(&mut self.alt_grid);
        if restore_cursor {
            (self.cx, self.cy) = self.saved_cursor;
            self.cx = self.cx.min(self.cols - 1);
            self.cy = self.cy.min(self.rows - 1);
        }
        self.alt_active = false;
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cx, self.cy)
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Resize the visible grid, preserving the overlapping top-left content.
    /// Cursor is clamped into the new bounds; scrollback is untouched.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let mut grid = vec![vec![Cell::blank(); cols]; rows];
        for (y, row) in grid.iter_mut().enumerate() {
            if let Some(old) = self.grid.get(y) {
                let n = row.len().min(old.len());
                row[..n].copy_from_slice(&old[..n]);
            }
        }
        self.grid = grid;
        self.cols = cols;
        self.rows = rows;
        self.cx = self.cx.min(cols - 1);
        self.cy = self.cy.min(rows - 1);
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        if self.alt_active {
            // self.grid is alt content, self.alt_grid is main: resize both.
            let mut main = vec![vec![Cell::blank(); cols]; rows];
            for (y, row) in main.iter_mut().enumerate() {
                if let Some(old) = self.alt_grid.get(y) {
                    let n = row.len().min(old.len());
                    row[..n].copy_from_slice(&old[..n]);
                }
            }
            self.alt_grid = main;
        }
    }

    pub fn cell(&self, x: usize, y: usize) -> Option<Cell> {
        self.grid.get(y)?.get(x).copied()
    }

    pub fn line_text(&self, y: usize) -> String {
        self.grid
            .get(y)
            .map(|row| row.iter().map(|c| c.c).collect())
            .unwrap_or_default()
    }

    /// DECSET 2004 state: when the hosted app set it, pastes must be
    /// wrapped in 200~/201~ so newlines insert instead of executing.
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Wheel position: 0 shows the live grid, N the N lines above it.
    pub fn view_offset(&self) -> usize {
        self.view_offset
    }

    /// Wheel up: look N lines further into history. No-op on alt
    /// (the window forwards the wheel to the app as arrows instead).
    pub fn scroll_view_up(&mut self, n: usize) {
        if self.alt_active {
            return;
        }
        self.view_offset = (self.view_offset + n).min(self.scrollback_len());
    }

    /// Wheel down: back toward live. Typing calls snap_to_bottom.
    pub fn scroll_view_down(&mut self, n: usize) {
        if self.alt_active {
            return;
        }
        self.view_offset = self.view_offset.saturating_sub(n);
    }

    /// Back to live (any keypress).
    pub fn snap_to_bottom(&mut self) {
        self.view_offset = 0;
    }

    /// Visible row y with the wheel applied: history lines above live,
    /// then the live grid. None past the top (over-scroll clamps, so
    /// this only fires on short grids).
    fn visible_row(&self, y: usize) -> Option<&Vec<Cell>> {
        let idx = self.scrollback_len().saturating_sub(self.view_offset) + y;
        if idx < self.scrollback_len() {
            self.scrollback.get(idx)
        } else {
            self.grid.get(idx - self.scrollback_len())
        }
    }

    pub fn visible_cell(&self, x: usize, y: usize) -> Option<Cell> {
        self.visible_row(y)?.get(x).copied()
    }

    pub fn visible_line_text(&self, y: usize) -> String {
        self.visible_row(y)
            .map(|row| row.iter().map(|c| c.c).collect())
            .unwrap_or_default()
    }

    /// Absolute-row base for the wheel-shifted view (selection math).
    pub fn visible_base(&self) -> usize {
        self.abs_base().saturating_sub(self.view_offset)
    }

    /// Alt screen active (vim/helix): the visible grid is NOT the main
    /// grid, so absolute-row math (selection, markers) must use offset 0
    /// and main-buffer text. The scrollback itself is preserved underneath.
    pub fn alt_active(&self) -> bool {
        self.alt_active
    }

    /// Absolute-row base for the visible grid: scrollback length on the
    /// main grid, 0 on alt (which shows its own blank grid).
    pub fn abs_base(&self) -> usize {
        if self.alt_active {
            0
        } else {
            self.scrollback.len()
        }
    }

    pub fn scrollback_line(&self, i: usize) -> Option<String> {
        self.scrollback
            .get(i)
            .map(|row| row.iter().map(|c| c.c).collect())
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn drain_markers(&mut self) -> Vec<Marker> {
        std::mem::take(&mut self.markers)
    }
}
