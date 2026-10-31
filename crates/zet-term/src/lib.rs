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
    pub reverse: bool,
}

impl Cell {
    fn blank() -> Self {
        Cell {
            c: ' ',
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
            reverse: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pen {
    fg: Color,
    bg: Color,
    bold: bool,
    reverse: bool,
}

impl Pen {
    fn reset(&mut self) {
        *self = Pen {
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
            reverse: false,
        };
    }

    fn sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.reset();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.reset(),
                1 => self.bold = true,
                7 => self.reverse = true,
                22 => self.bold = false,
                27 => self.reverse = false,
                30..=37 => self.fg = Color::Indexed((params[i] - 30) as u8),
                38 => {
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.fg = Color::Indexed(params[i + 2] as u8);
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        self.fg = Color::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                39 => self.fg = Color::Default,
                40..=47 => self.bg = Color::Indexed((params[i] - 40) as u8),
                48 => {
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.bg = Color::Indexed(params[i + 2] as u8);
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        self.bg = Color::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                49 => self.bg = Color::Default,
                90..=97 => self.fg = Color::Indexed((params[i] - 90 + 8) as u8),
                100..=107 => self.bg = Color::Indexed((params[i] - 100 + 8) as u8),
                _ => {}
            }
            i += 1;
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
    /// ESC with intermediate bytes (ESC # 8, ESC % G, ESC space F …): the
    /// whole sequence is consumed through its final byte. Without this the
    /// argument byte leaks onto the grid as text — found by the vttest
    /// corpus, which showed `ESC # 8` printing a stray `8`.
    EscIntermediate,
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
    /// Pending terminal responses to write back to the PTY.
    responses: Vec<Vec<u8>>,
    /// First time bytes were fed to this terminal (perf instrumentation).
    first_output: Option<std::time::Instant>,
    first_output_reported: bool,
    perf_enabled: bool,
    /// Configured default foreground/background colors used for OSC 10/11
    /// color queries. These are RGB triples, not palette indices.
    default_fg: (u8, u8, u8),
    default_bg: (u8, u8, u8),
    utf8_buf: [u8; 4],
    utf8_need: u8,
    utf8_have: u8,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback(cols, rows, 10_000)
    }

    pub fn with_perf(cols: usize, rows: usize, perf_enabled: bool) -> Self {
        Self::with_scrollback_and_perf(cols, rows, 10_000, perf_enabled)
    }

    pub fn with_scrollback_and_perf(
        cols: usize,
        rows: usize,
        scrollback_cap: usize,
        perf_enabled: bool,
    ) -> Self {
        let mut terminal = Self::with_scrollback(cols, rows, scrollback_cap);
        terminal.perf_enabled = perf_enabled;
        terminal
    }

    pub fn new_with_perf(cols: usize, rows: usize, perf_enabled: bool) -> Self {
        Self::with_perf(cols, rows, perf_enabled)
    }

    pub fn with_perf_scrollback(
        cols: usize,
        rows: usize,
        scrollback_cap: usize,
        perf_enabled: bool,
    ) -> Self {
        Self::with_scrollback_and_perf(cols, rows, scrollback_cap, perf_enabled)
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
                reverse: false,
            },
            state: State::Ground,
            osc_buf: Vec::new(),
            events: Vec::new(),
            markers: Vec::new(),
            responses: Vec::new(),
            first_output: None,
            first_output_reported: false,
            perf_enabled: false,
            default_fg: (255, 255, 255),
            default_bg: (0, 0, 0),
            utf8_buf: [0; 4],
            utf8_need: 0,
            utf8_have: 0,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        if self.first_output.is_none()
            && !bytes.is_empty()
            && std::env::var_os("ZET_PERF").is_some()
        {
            self.first_output = Some(std::time::Instant::now());
        }
        for &b in bytes {
            match self.state {
                State::Ground => self.ground(b),
                State::Escape => self.escape(b),
                State::Scs => self.state = State::Ground,
                State::EscIntermediate => {
                    // Final byte ends the sequence; another intermediate
                    // continues it.
                    self.state = if (0x20..=0x2f).contains(&b) {
                        State::EscIntermediate
                    } else {
                        State::Ground
                    };
                }
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
        } else if (0x20..=0x2f).contains(&b) {
            // Any other ESC intermediate: consume through the final byte.
            self.state = State::EscIntermediate;
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
        if (code == 10 || code == 11) && body == "?" {
            let color = if code == 10 {
                self.default_fg
            } else {
                self.default_bg
            };
            self.push_color_response(code, color);
        }
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
            b'n' if params.first().copied() == Some(5) => {
                self.responses.push(b"\x1b[0n".to_vec());
            }
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
            b'J' => match params.first().copied().unwrap_or(0) {
                2 => {
                    self.grid = vec![vec![Cell::blank(); self.cols]; self.rows];
                    self.cx = 0;
                    self.cy = 0;
                }
                // Cursor to end of screen. Found by the vttest corpus: only
                // ED 2 was handled, so `clear` from mid-screen was a no-op
                // and left stale text behind.
                0 => {
                    let from = self.cx.min(self.cols);
                    self.grid[self.cy][from..].fill(Cell::blank());
                    for y in (self.cy + 1)..self.rows {
                        self.grid[y].fill(Cell::blank());
                    }
                }
                _ => {}
            },
            b'K' => {
                let row = &mut self.grid[self.cy];
                match params.first().copied().unwrap_or(0) {
                    1 => row[..=self.cx.min(self.cols - 1)].fill(Cell::blank()),
                    2 => row.fill(Cell::blank()),
                    _ => row[self.cx.min(self.cols)..].fill(Cell::blank()),
                }
            }
            // IL/DL: found by the vttest corpus. Absent before, so a
            // full-screen editor's line insert/delete silently corrupted the
            // display. Both leave the cursor where it is (VT100 only moves
            // lines within the region).
            b'L' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.edit_lines(self.cy, n, true);
            }
            b'M' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.edit_lines(self.cy, n, false);
            }
            _ => {}
        }
    }

    /// Insert (`insert = true`) or delete (`false`) `n` lines at row `y`,
    /// bounded by the scroll region.
    ///
    /// Insert pushes the lines below *down* by `n` and opens blanks at `y`;
    /// delete pulls them *up* by `n` and closes blanks at the far edge. In
    /// both cases lines pushed past the region are dropped, never into
    /// scrollback: this edits in place, unlike a real scroll.
    fn edit_lines(&mut self, y: usize, n: usize, insert: bool) {
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        if y < top || y > bottom {
            return;
        }
        let n = n.min(bottom - y + 1);
        if insert {
            for row in ((y + n)..=bottom).rev() {
                let from = self.grid[row - n].clone();
                self.grid[row] = from;
            }
            for row in y..(y + n) {
                self.grid[row].fill(Cell::blank());
            }
        } else {
            for row in y..=(bottom - n) {
                let from = self.grid[row + n].clone();
                self.grid[row] = from;
            }
            for row in (bottom + 1 - n)..=bottom {
                self.grid[row].fill(Cell::blank());
            }
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
            reverse: self.pen.reverse,
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

    /// First time bytes were fed, if any.
    pub fn first_output_time(&self) -> Option<std::time::Instant> {
        self.first_output
    }
    pub fn set_perf_enabled(&mut self, enabled: bool) {
        self.perf_enabled = enabled;
    }

    pub fn take_first_output_time(&mut self) -> Option<std::time::Instant> {
        if self.first_output_reported {
            None
        } else {
            self.first_output_reported = true;
            self.first_output
        }
    }

    pub fn first_output_reported(&self) -> bool {
        self.first_output_reported
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

    /// Case-insensitive substring search over scrollback + live grid.
    /// Returns (absolute row, char col, char len), oldest first,
    /// capped so a huge buffer can't stall a keystroke. Matching runs
    /// in char space throughout: lowercasing can change byte length
    /// (ẞ/İ), so byte offsets must never slice the original text.
    pub fn search_matches(&self, query: &str) -> Vec<(usize, usize, usize)> {
        const CAP: usize = 1000;
        if query.is_empty() {
            return Vec::new();
        }
        let q: Vec<char> = query.to_lowercase().chars().collect();
        let mut out = Vec::new();
        let push_line = |abs: usize, text: &str, out: &mut Vec<(usize, usize, usize)>| {
            // Lowercase expansion (İ → i + combining dot) shifts char
            // positions: align every lowered char back to its original
            // grid cell so reported cols match the screen.
            let mut lower: Vec<char> = Vec::new();
            let mut idx: Vec<usize> = Vec::new();
            for (i, c) in text.chars().enumerate() {
                for lc in c.to_lowercase() {
                    lower.push(lc);
                    idx.push(i);
                }
            }
            if q.len() > lower.len() {
                return;
            }
            for (p, window) in lower.windows(q.len()).enumerate() {
                if out.len() >= CAP {
                    break;
                }
                if window == q.as_slice() {
                    let col = idx[p];
                    let len = idx[p + q.len() - 1] - col + 1;
                    out.push((abs, col, len));
                }
            }
        };
        // Alt screen: grid only (scrollback is stale, reveal no-ops).
        if !self.alt_active {
            for (i, row) in self.scrollback.iter().enumerate() {
                let text: String = row.iter().map(|c| c.c).collect();
                push_line(i, text.trim_end(), &mut out);
                if out.len() >= CAP {
                    return out;
                }
            }
        }
        for y in 0..self.rows {
            let abs = self.abs_base() + y;
            push_line(abs, self.line_text(y).trim_end(), &mut out);
            if out.len() >= CAP {
                break;
            }
        }
        out
    }

    /// Scroll so absolute row r lands mid-view (no-op when already
    /// visible or on the alt screen).
    pub fn reveal_row(&mut self, r: usize) {
        if self.alt_active {
            return;
        }
        let base = self.abs_base();
        // Visible absolute rows are [base - view_offset, +rows).
        let vis_top = base.saturating_sub(self.view_offset);
        let vis_bot = vis_top + self.rows;
        if r >= vis_top && r < vis_bot {
            return;
        }
        let want_top = r.saturating_sub(self.rows / 2);
        self.view_offset = base.saturating_sub(want_top).min(self.scrollback.len());
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn drain_markers(&mut self) -> Vec<Marker> {
        std::mem::take(&mut self.markers)
    }

    /// Set the terminal's configured default foreground/background colors.
    /// OSC 10/11 color queries respond with these values.
    pub fn set_default_colors(&mut self, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        self.default_fg = fg;
        self.default_bg = bg;
    }

    /// Responses generated while feeding bytes. The caller must write these
    /// back to the PTY; they are not fed back into this terminal.
    pub fn drain_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.responses)
    }

    /// Generate an OSC color response. `code` is 10 (foreground) or 11
    /// (background). Channels are reported as 16-bit hexadecimal values.
    fn push_color_response(&mut self, code: u16, color: (u8, u8, u8)) {
        let (r, g, b) = color;
        self.responses.push(
            format!(
                "\x1b]{};rgb:{:04x}/{:04x}/{:04x}\x07",
                code,
                r as u16 * 0x0101,
                g as u16 * 0x0101,
                b as u16 * 0x0101
            )
            .into_bytes(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc11_query_responds_with_background_color() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b]11;?\x07");
        assert_eq!(
            term.drain_responses(),
            vec![b"\x1b]11;rgb:0000/0000/0000\x07".to_vec()]
        );
        assert_eq!(term.cell(0, 0).unwrap().c, ' ');
    }

    #[test]
    fn osc10_query_responds_with_foreground_color() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b]10;?\x07");
        assert_eq!(
            term.drain_responses(),
            vec![b"\x1b]10;rgb:ffff/ffff/ffff\x07".to_vec()]
        );
    }

    #[test]
    fn osc_color_query_accepts_st_terminator() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b]11;?\x1b\\");
        assert_eq!(
            term.drain_responses(),
            vec![b"\x1b]11;rgb:0000/0000/0000\x07".to_vec()]
        );
    }

    #[test]
    fn dsr_status_report_responds_ok() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b[5n");
        assert_eq!(term.drain_responses(), vec![b"\x1b[0n".to_vec()]);
        assert_eq!(term.cell(0, 0).unwrap().c, ' ');
    }

    #[test]
    fn fragmented_osc11_query_responds_once() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b]11;");
        assert!(term.drain_responses().is_empty());
        term.feed(b"?");
        assert!(term.drain_responses().is_empty());
        term.feed(b"\x07");
        assert_eq!(
            term.drain_responses(),
            vec![b"\x1b]11;rgb:0000/0000/0000\x07".to_vec()]
        );
        assert!(term.drain_responses().is_empty());
    }

    #[test]
    fn malformed_color_queries_do_not_respond() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b]11;red\x07");
        term.feed(b"\x1b]12;?\x07");
        term.feed(b"plain text");
        assert!(term.drain_responses().is_empty());
    }

    #[test]
    fn terminal_responses_do_not_generate_feedback() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b]11;rgb:0000/0000/0000\x07");
        term.feed(b"\x1b[0n");
        assert!(term.drain_responses().is_empty());
    }

    #[test]
    fn set_default_colors_changes_osc_responses() {
        let mut term = Terminal::new(80, 24);
        term.set_default_colors((255, 128, 0), (10, 20, 30));
        term.feed(b"\x1b]10;?\x07");
        term.feed(b"\x1b]11;?\x07");
        let responses = term.drain_responses();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0], b"\x1b]10;rgb:ffff/8080/0000\x07");
        assert_eq!(responses[1], b"\x1b]11;rgb:0a0a/1414/1e1e\x07");
    }
}
