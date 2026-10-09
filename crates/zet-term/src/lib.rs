use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
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
                _ => {}
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Csi {
        current: u16,
        params: [u16; 16],
        count: usize,
    },
}

pub struct Terminal {
    cols: usize,
    rows: usize,
    grid: Vec<Vec<Cell>>,
    scrollback: VecDeque<Vec<Cell>>,
    scrollback_cap: usize,
    cx: usize,
    cy: usize,
    pen: Pen,
    state: State,
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
            cx: 0,
            cy: 0,
            pen: Pen {
                fg: Color::Default,
                bg: Color::Default,
                bold: false,
            },
            state: State::Ground,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match self.state {
                State::Ground => self.ground(b),
                State::Escape => self.escape(b),
                State::Csi {
                    mut current,
                    mut params,
                    mut count,
                } => {
                    if b.is_ascii_digit() {
                        current = current.saturating_mul(10).saturating_add((b - b'0') as u16);
                        self.state = State::Csi {
                            current,
                            params,
                            count,
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
                        };
                    } else if (0x40..=0x7e).contains(&b) {
                        if count < params.len() {
                            params[count] = current;
                            count += 1;
                        }
                        self.dispatch_csi(b, &params[..count]);
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
            self.linefeed();
            self.cx = 0;
        } else if b == b'\r' {
            self.cx = 0;
        } else if b == 0x1b {
            self.state = State::Escape;
        } else if b >= 0x20 {
            self.put(b as char);
        }
    }

    fn escape(&mut self, b: u8) {
        if b == b'[' {
            self.state = State::Csi {
                current: 0,
                params: [0; 16],
                count: 0,
            };
        } else {
            self.state = State::Ground;
        }
    }

    fn dispatch_csi(&mut self, final_byte: u8, params: &[u16]) {
        match final_byte {
            b'm' => self.pen.sgr(params),
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
        self.grid[self.cy][self.cx] = Cell {
            c,
            fg: self.pen.fg,
            bg: self.pen.bg,
            bold: self.pen.bold,
        };
        self.cx += 1;
        if self.cx >= self.cols {
            self.cx = 0;
            self.linefeed();
        }
    }

    fn linefeed(&mut self) {
        if self.cy + 1 >= self.rows {
            self.scroll_up();
        } else {
            self.cy += 1;
        }
    }

    fn scroll_up(&mut self) {
        let top = self.grid[0].clone();
        self.grid.remove(0);
        self.grid.push(vec![Cell::blank(); self.cols]);
        if self.scrollback_cap > 0 {
            self.scrollback.push_back(top);
            while self.scrollback.len() > self.scrollback_cap {
                self.scrollback.pop_front();
            }
        }
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cx, self.cy)
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

    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    pub fn scrollback_line(&self, i: usize) -> Option<String> {
        self.scrollback
            .get(i)
            .map(|row| row.iter().map(|c| c.c).collect())
    }
}
