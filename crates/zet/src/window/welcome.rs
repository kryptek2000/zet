//! P2-B — the app-owned ZET signature identity.
//!
//! The identity is decorative chrome: it is planned from genuinely free
//! terminal rows and drawn through the native pixel presentation stream. It
//! never writes to the terminal grid, never repaints terminal cells, and
//! never fabricates PTY output. A normal shell prompt, startup control
//! sequences, and background output elsewhere on the screen all leave it
//! intact; interaction or output that reaches the reserved rows retires it.

use crate::window::panels::{
    signature_wordmark_lines, SignatureIdentitySize, SIGNATURE_BRAND_COPY, SIGNATURE_MINIMAL_TEXT,
    SIGNATURE_SUPPORTING_IDENTITY, SIGNATURE_TAGLINE,
};
use zet_gpu::presentation::{UiFontFamily, UiTextStyle};
use zet_gpu::quad::Rgba;
use zet_gpu::renderer::Viewport;
use zet_gpu::Renderer;
use zet_term::{Color, Terminal};

/// Cell width of the approved eight-line wordmark.
pub const WORDMARK_COLS: u32 = 48;
/// Rows used by the full identity: wordmark, spacer, and three copy lines.
pub const FULL_ROWS: u32 = 12;
/// Rows used by the compact identity: wordmark, spacer, and brand copy.
pub const COMPACT_ROWS: u32 = 10;
/// Minimum terminal width for the one-line minimal identity.
pub const MINIMAL_COLS: u32 = 16;

/// A cell-aligned rectangle reserved by the identity, in viewport-relative
/// terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WelcomeRect {
    pub col: u32,
    pub row: u32,
    pub width: u32,
    pub height: u32,
}

impl WelcomeRect {
    pub const fn right(self) -> u32 {
        self.col.saturating_add(self.width)
    }

    pub const fn bottom(self) -> u32 {
        self.row.saturating_add(self.height)
    }

    pub const fn contains(self, col: u32, row: u32) -> bool {
        col >= self.col && col < self.right() && row >= self.row && row < self.bottom()
    }
}

/// The semantic ink role of one identity line. Colors stay in the theme so
/// the identity follows the Aperture Instrument palette instead of baking in
/// fixed RGB values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeLineRole {
    Wordmark,
    Brand,
    Tagline,
    Supporting,
    Minimal,
}

impl WelcomeLineRole {
    fn style_and_ink(self, cell_w: u32, cell_h: u32) -> (UiTextStyle, Rgba) {
        let cw = cell_w.max(1) as f32;
        let ch = cell_h.max(1) as f32;
        match self {
            Self::Wordmark => {
                // Monospace advance is ~0.6em, so this makes one wordmark
                // character occupy one terminal cell while staying inside
                // the row's vertical bounds.
                let size = (cw / 0.6).min(ch * 0.92).max(1.0);
                (
                    UiTextStyle::new(size, ch, 500, 0.0, UiFontFamily::Monospace),
                    zet_gpu::renderer::PANEL_CYAN,
                )
            }
            Self::Brand => (
                UiTextStyle::new((ch * 0.58).max(1.0), ch, 600, 0.4, UiFontFamily::Monospace),
                zet_gpu::renderer::PANEL_TEXT,
            ),
            Self::Tagline => (
                UiTextStyle::new((ch * 0.54).max(1.0), ch, 500, 0.3, UiFontFamily::Monospace),
                zet_gpu::renderer::PANEL_SECONDARY,
            ),
            Self::Supporting => (
                UiTextStyle::new((ch * 0.50).max(1.0), ch, 500, 0.2, UiFontFamily::Monospace),
                zet_gpu::renderer::PANEL_MUTED,
            ),
            Self::Minimal => {
                let size = (cw / 0.6).min(ch * 0.82).max(1.0);
                (
                    UiTextStyle::new(size, ch, 600, 0.6, UiFontFamily::Monospace),
                    zet_gpu::renderer::PANEL_CYAN,
                )
            }
        }
    }
}

/// One rendered identity line, positioned relative to the terminal viewport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeLine {
    pub row: u32,
    pub text: &'static str,
    pub role: WelcomeLineRole,
}

/// A resolved identity presentation. `rect` is the exact reserved cell
/// region; `lines` are the native text commands that produce the artwork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomePresentation {
    pub size: SignatureIdentitySize,
    pub rect: WelcomeRect,
    pub lines: Vec<WelcomeLine>,
    pub viewport_cols: u32,
    pub viewport_rows: u32,
}

/// One staged native presentation command. Kept renderer-independent so the
/// production draw path can be exercised without a GPU.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WelcomeDrawCommand {
    pub x: f32,
    pub baseline: f32,
    pub text: &'static str,
    pub style: UiTextStyle,
    pub ink: Rgba,
    pub max_width: u32,
}

impl WelcomePresentation {
    /// Build the exact native text commands used by the application. The
    /// caller supplies the production glyph measurement function, so tests
    /// can assert placement and emitted lines without a display.
    pub fn draw_commands<F>(
        &self,
        viewport: Viewport,
        cell_px: (u32, u32),
        mut measure: F,
    ) -> Vec<WelcomeDrawCommand>
    where
        F: FnMut(&str, UiTextStyle) -> u32,
    {
        if viewport.cols == 0 || viewport.rows == 0 {
            return Vec::new();
        }
        let cw = cell_px.0.max(1);
        let ch = cell_px.1.max(1);
        let left = viewport.x.saturating_mul(cw) as f32;
        let width = viewport.cols.saturating_mul(cw);
        let wordmark_left = left + self.rect.col.saturating_mul(cw) as f32;
        let wordmark_width = self.rect.width.saturating_mul(cw).min(width);
        let mut commands = Vec::new();
        for line in &self.lines {
            if line.row >= viewport.rows {
                continue;
            }
            let (style, ink) = line.role.style_and_ink(cw, ch);
            let (x, max_width) = match line.role {
                // Every wordmark row starts at the same cell so the approved
                // leading spaces and relative letter geometry are preserved.
                WelcomeLineRole::Wordmark => (wordmark_left, wordmark_width),
                _ => {
                    let measured = measure(line.text, style).min(width);
                    (left + (width.saturating_sub(measured) as f32 / 2.0), width)
                }
            };
            let top = viewport.y.saturating_add(line.row).saturating_mul(ch) as f32;
            let baseline =
                top + ((ch as f32 - style.line_height_px).max(0.0) * 0.5) + style.baseline_offset();
            commands.push(WelcomeDrawCommand {
                x,
                baseline,
                text: line.text,
                style,
                ink,
                max_width,
            });
        }
        commands
    }
}

/// Draw the identity through the native UI presentation stream. This appends
/// glyph quads after the terminal frame; it never calls the terminal renderer
/// and never touches PTY cells.
pub fn draw(r: &mut Renderer, viewport: Viewport, presentation: &WelcomePresentation) {
    let cell_px = r.cell_px();
    let commands = presentation.draw_commands(viewport, cell_px, |text, style| {
        r.measure_ui_text_px(text, style)
    });
    for command in commands {
        r.draw_ui_text_px(
            command.x,
            command.baseline,
            command.text,
            command.style,
            command.ink,
            Some(command.max_width),
        );
    }
}

fn cell_is_content(cell: zet_term::Cell) -> bool {
    cell.c != ' ' || cell.bg != Color::Default || cell.reverse
}

/// Mark every visible row that already carries terminal content or the live
/// cursor. The identity may only use rows where this is false.
fn occupied_rows(term: &Terminal, cols: u32, rows: u32) -> Vec<bool> {
    let cursor = term.cursor();
    let cols = cols as usize;
    let rows = rows as usize;
    let mut occupied = vec![false; rows];
    for (y, slot) in occupied.iter_mut().enumerate() {
        if cursor.1 == y {
            *slot = true;
            continue;
        }
        *slot = (0..cols).any(|x| term.visible_cell(x, y).is_some_and(cell_is_content));
    }
    occupied
}

/// Maximal runs of free rows. One blank row of breathing room is kept next
/// to existing content (but not against the viewport edge), so the identity
/// never sits flush against a prompt or output line.
fn free_runs(occupied: &[bool]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (i, &row_occupied) in occupied.iter().enumerate() {
        match (row_occupied, start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                if s < i {
                    runs.push((s, i));
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if s < occupied.len() {
            runs.push((s, occupied.len()));
        }
    }
    runs.into_iter()
        .filter_map(|(mut s, mut e)| {
            if s > 0 && occupied[s - 1] {
                s += 1;
            }
            if e < occupied.len() && occupied[e] {
                e = e.saturating_sub(1);
            }
            (s < e).then_some((s, e))
        })
        .collect()
}

/// Pick the free run whose centred placement is closest to the viewport's
/// vertical centre. Ties resolve to the earliest run for deterministic
/// behaviour.
fn best_top(runs: &[(usize, usize)], need: usize, viewport_rows: usize) -> Option<usize> {
    runs.iter()
        .filter(|(start, end)| end.saturating_sub(*start) >= need)
        .map(|(start, end)| {
            let top = start + (end - start - need) / 2;
            (top, (top * 2 + need).abs_diff(viewport_rows))
        })
        .min_by_key(|(_, distance)| *distance)
        .map(|(top, _)| top)
}

fn build_lines(size: SignatureIdentitySize, top: usize) -> Vec<WelcomeLine> {
    let mut lines = Vec::new();
    let push = |lines: &mut Vec<WelcomeLine>, offset: usize, text: &'static str, role| {
        lines.push(WelcomeLine {
            row: top.saturating_add(offset) as u32,
            text,
            role,
        });
    };
    match size {
        SignatureIdentitySize::Full => {
            for (offset, line) in signature_wordmark_lines(size).iter().enumerate() {
                push(&mut lines, offset, line, WelcomeLineRole::Wordmark);
            }
            push(&mut lines, 9, SIGNATURE_BRAND_COPY, WelcomeLineRole::Brand);
            push(&mut lines, 10, SIGNATURE_TAGLINE, WelcomeLineRole::Tagline);
            push(
                &mut lines,
                11,
                SIGNATURE_SUPPORTING_IDENTITY,
                WelcomeLineRole::Supporting,
            );
        }
        SignatureIdentitySize::Compact => {
            for (offset, line) in signature_wordmark_lines(size).iter().enumerate() {
                push(&mut lines, offset, line, WelcomeLineRole::Wordmark);
            }
            push(&mut lines, 9, SIGNATURE_BRAND_COPY, WelcomeLineRole::Brand);
        }
        SignatureIdentitySize::Minimal => {
            push(
                &mut lines,
                0,
                SIGNATURE_MINIMAL_TEXT,
                WelcomeLineRole::Minimal,
            );
        }
        SignatureIdentitySize::Omitted => {}
    }
    lines
}

/// Resolve a safe identity for the current terminal content.
///
/// The scan uses real visible cells and the live cursor. Full identity wins
/// when twelve free rows exist, then compact at ten, then the one-line
/// minimal mark; otherwise the artwork is omitted rather than clipped.
pub fn plan(
    term: &Terminal,
    viewport_cols: u32,
    viewport_rows: u32,
) -> Option<WelcomePresentation> {
    if term.alt_active() || term.view_offset() != 0 {
        return None;
    }
    let cols = viewport_cols.min(term.cols() as u32);
    let rows = viewport_rows.min(term.rows() as u32);
    if cols == 0 || rows == 0 {
        return None;
    }
    let occupancy = occupied_rows(term, cols, rows);
    let runs = free_runs(&occupancy);

    let candidates = [
        (
            SignatureIdentitySize::Full,
            FULL_ROWS as usize,
            WORDMARK_COLS,
            WORDMARK_COLS,
        ),
        (
            SignatureIdentitySize::Compact,
            COMPACT_ROWS as usize,
            WORDMARK_COLS,
            WORDMARK_COLS,
        ),
        (
            SignatureIdentitySize::Minimal,
            1usize,
            SIGNATURE_MINIMAL_TEXT.chars().count() as u32,
            MINIMAL_COLS,
        ),
    ];
    for (size, height, width, min_cols) in candidates {
        if width > cols || cols < min_cols {
            continue;
        }
        let Some(top) = best_top(&runs, height, rows as usize) else {
            continue;
        };
        let col = (cols - width) / 2;
        return Some(WelcomePresentation {
            size,
            rect: WelcomeRect {
                col,
                row: top as u32,
                width,
                height: height as u32,
            },
            lines: build_lines(size, top),
            viewport_cols,
            viewport_rows,
        });
    }
    None
}

/// True when terminal content, the cursor, scrolling, or a full-screen app
/// has reached a previously reserved identity region.
pub fn rect_conflicts(term: &Terminal, rect: &WelcomeRect) -> bool {
    if term.alt_active() || term.view_offset() != 0 {
        return true;
    }
    if rect.right() > term.cols() as u32 || rect.bottom() > term.rows() as u32 {
        return true;
    }
    let (cursor_col, cursor_row) = term.cursor();
    if rect.contains(cursor_col as u32, cursor_row as u32) {
        return true;
    }
    (rect.row as usize..rect.bottom() as usize).any(|y| {
        (rect.col as usize..rect.right() as usize)
            .any(|x| term.visible_cell(x, y).is_some_and(cell_is_content))
    })
}

/// Per-run welcome state. `present` is the production entry point: it plans
/// on first visibility, keeps a stable placement, and retires the identity
/// permanently once interaction or conflicting output arrives.
#[derive(Debug, Clone)]
pub struct WelcomeIdentity {
    disabled: bool,
    dismissed: bool,
    anchor: Option<WelcomePresentation>,
}

impl WelcomeIdentity {
    pub fn new(disabled: bool) -> Self {
        Self {
            disabled,
            dismissed: false,
            anchor: None,
        }
    }

    pub fn disabled_from_env() -> bool {
        std::env::var_os("ZET_NO_WELCOME").is_some()
    }

    #[cfg(test)]
    pub fn disabled(&self) -> bool {
        self.disabled
    }

    #[cfg(test)]
    pub fn dismissed(&self) -> bool {
        self.dismissed
    }

    /// Retire the identity for the rest of this run.
    pub fn dismiss(&mut self) {
        self.dismissed = true;
        self.anchor = None;
    }

    /// Drop the current placement so the next frame can re-plan after a
    /// resize or layout change. This does not dismiss the identity.
    pub fn release_placement(&mut self) {
        self.anchor = None;
    }

    /// Return the identity to present this frame, if any. Planning and
    /// conflict detection happen here, before the renderer is touched, so a
    /// frame can never paint the identity over terminal content.
    pub fn present(
        &mut self,
        term: &Terminal,
        viewport_cols: u32,
        viewport_rows: u32,
    ) -> Option<&WelcomePresentation> {
        if self.disabled {
            self.anchor = None;
            return None;
        }
        if self.dismissed {
            return None;
        }
        if term.alt_active() || term.view_offset() != 0 {
            self.dismiss();
            return None;
        }

        let anchor_state = match self.anchor.as_ref() {
            Some(anchor)
                if anchor.viewport_cols == viewport_cols
                    && anchor.viewport_rows == viewport_rows =>
            {
                Some(rect_conflicts(term, &anchor.rect))
            }
            Some(_) => None,
            None => None,
        };
        match anchor_state {
            Some(false) => return self.anchor.as_ref(),
            Some(true) => {
                self.dismiss();
                return None;
            }
            None => self.anchor = None,
        }

        let presentation = plan(term, viewport_cols, viewport_rows)?;
        self.anchor = Some(presentation);
        self.anchor.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::panels::SIGNATURE_WORDMARK;

    fn prompt_terminal() -> Terminal {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b[2J\x1b[Huser@host:~$ ");
        term
    }

    fn free_of_content(term: &Terminal, presentation: &WelcomePresentation) -> bool {
        let rect = presentation.rect;
        let (cursor_col, cursor_row) = term.cursor();
        !rect.contains(cursor_col as u32, cursor_row as u32)
            && (rect.row as usize..rect.bottom() as usize).all(|y| {
                (rect.col as usize..rect.right() as usize).all(|x| {
                    term.visible_cell(x, y)
                        .is_none_or(|cell| !cell_is_content(cell))
                })
            })
    }

    #[test]
    fn normal_shell_prompt_keeps_large_identity_visible() {
        let term = prompt_terminal();
        let presentation = plan(&term, 80, 24).expect("identity with a live prompt");
        assert_eq!(presentation.size, SignatureIdentitySize::Full);
        assert!(presentation.rect.row >= 1, "prompt row must stay clear");
        assert!(free_of_content(&term, &presentation));
        assert_eq!(presentation.lines.len(), 11);
        assert_eq!(presentation.lines[0].text, SIGNATURE_WORDMARK[0]);
        assert_eq!(presentation.lines[8].text, SIGNATURE_BRAND_COPY);
        assert!(presentation.rect.bottom() <= 24);
    }

    #[test]
    fn anchor_set_before_prompt_survives_clear_and_prompt() {
        let mut term = Terminal::new(80, 24);
        let mut welcome = WelcomeIdentity::new(false);
        assert!(welcome.present(&term, 80, 24).is_some());
        // The shell finishes initialising after the first frame: startup
        // control output, clear, and the first prompt must not retire it.
        term.feed(b"\x1b]0;zet\x07\x1b[?2004h");
        term.feed(b"\x1b[2J\x1b[H\x1b[32muser@host\x1b[0m:~$ ");
        assert!(welcome.present(&term, 80, 24).is_some());
        assert!(!welcome.dismissed());
    }

    #[test]
    fn initial_control_output_does_not_dismiss() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b]0;zet\x07\x1b[?2004h\x1b[?25h");
        term.feed(b"\x1b[2J\x1b[H\x1b[32muser@host\x1b[0m:~$ ");
        let mut welcome = WelcomeIdentity::new(false);
        assert!(welcome.present(&term, 80, 24).is_some());
        assert!(!welcome.dismissed());
    }

    #[test]
    fn identity_survives_shell_startup_banner_and_clear() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"Last login: today on tty1\r\n");
        term.feed(b"\x1b[?2004h");
        term.feed(b"\x1b[2J\x1b[H\x1b[1;36muser@host\x1b[0m:~$ ");
        let presentation = plan(&term, 80, 24).expect("identity after shell init");
        assert_eq!(presentation.size, SignatureIdentitySize::Full);
        assert!(free_of_content(&term, &presentation));
    }

    #[test]
    fn identity_never_overlays_content_or_cursor() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b[2J\x1b[Htop\r\n");
        term.feed(b"\x1b[7;1Hmiddle output");
        term.feed(b"\x1b[20;1Hbottom");
        term.feed(b"\x1b[19;40H");
        let presentation = plan(&term, 80, 24).expect("identity in a free band");
        assert!(free_of_content(&term, &presentation));
        assert_eq!(presentation.size, SignatureIdentitySize::Minimal);
    }

    #[test]
    fn immediate_dismissal_on_interaction() {
        let term = prompt_terminal();
        let mut welcome = WelcomeIdentity::new(false);
        assert!(welcome.present(&term, 80, 24).is_some());
        welcome.dismiss();
        assert!(welcome.dismissed());
        assert!(welcome.present(&term, 80, 24).is_none());
    }

    #[test]
    fn full_screen_application_entry_hides_identity() {
        let mut term = prompt_terminal();
        let mut welcome = WelcomeIdentity::new(false);
        assert!(welcome.present(&term, 80, 24).is_some());
        term.feed(b"\x1b[?1049h");
        assert!(welcome.present(&term, 80, 24).is_none());
        assert!(welcome.dismissed());
    }

    #[test]
    fn background_output_in_reserved_region_dismisses() {
        let mut term = prompt_terminal();
        let mut welcome = WelcomeIdentity::new(false);
        let rect = welcome
            .present(&term, 80, 24)
            .expect("identity before background output")
            .rect;
        let x = rect.col + rect.width / 2;
        let y = rect.row + rect.height / 2;
        term.feed(format!("\x1b[{};{}HX", y + 1, x + 1).as_bytes());
        term.feed(b"\x1b[1;1H");
        assert!(welcome.present(&term, 80, 24).is_none());
        assert!(welcome.dismissed());
    }

    #[test]
    fn output_outside_reserved_region_does_not_dismiss() {
        let mut term = prompt_terminal();
        let mut welcome = WelcomeIdentity::new(false);
        let rect = welcome
            .present(&term, 80, 24)
            .expect("identity before background output")
            .rect;
        let outside_row = if rect.row > 1 { 1 } else { rect.bottom() };
        term.feed(format!("\x1b[{};1Hbackground", outside_row + 1).as_bytes());
        term.feed(b"\x1b[1;1H");
        assert!(welcome.present(&term, 80, 24).is_some());
        assert!(!welcome.dismissed());
    }

    #[test]
    fn narrow_window_uses_minimal_identity() {
        let term = prompt_terminal();
        let presentation = plan(&term, 20, 24).expect("minimal identity");
        assert_eq!(presentation.size, SignatureIdentitySize::Minimal);
        assert!(presentation.rect.right() <= 20);
        assert_eq!(presentation.lines.len(), 1);
        assert_eq!(presentation.lines[0].text, SIGNATURE_MINIMAL_TEXT);
        assert!(plan(&term, 10, 24).is_none());
    }

    #[test]
    fn compact_and_minimal_vertical_bounds() {
        let mut term = Terminal::new(80, 12);
        term.feed(b"\x1b[2J\x1b[H$ ");
        let presentation = plan(&term, 80, 12).expect("compact identity");
        assert_eq!(presentation.size, SignatureIdentitySize::Compact);
        assert_eq!(presentation.rect.height, COMPACT_ROWS);
        assert!(presentation.rect.bottom() <= 12);

        let mut small = Terminal::new(80, 10);
        small.feed(b"\x1b[2J\x1b[H$ ");
        let presentation = plan(&small, 80, 10).expect("minimal fallback");
        assert_eq!(presentation.size, SignatureIdentitySize::Minimal);
        assert_eq!(presentation.rect.height, 1);
        assert!(presentation.rect.bottom() <= 10);
    }

    #[test]
    fn resize_replans_without_dismissing() {
        let mut term = prompt_terminal();
        let mut welcome = WelcomeIdentity::new(false);
        assert_eq!(
            welcome.present(&term, 80, 24).unwrap().size,
            SignatureIdentitySize::Full
        );
        term.resize(80, 12);
        let compact = welcome.present(&term, 80, 12).expect("replanned identity");
        assert_eq!(compact.size, SignatureIdentitySize::Compact);
        assert!(compact.rect.bottom() <= 12);
        assert!(!welcome.dismissed());
        term.resize(80, 10);
        let minimal = welcome.present(&term, 80, 10).expect("narrow height");
        assert_eq!(minimal.size, SignatureIdentitySize::Minimal);
        assert!(minimal.rect.bottom() <= 10);
    }

    #[test]
    fn disabled_configuration_never_plans() {
        let term = prompt_terminal();
        let mut welcome = WelcomeIdentity::new(true);
        assert!(welcome.disabled());
        assert!(welcome.present(&term, 80, 24).is_none());
        assert!(!welcome.dismissed());
    }

    #[test]
    fn presentation_does_not_mutate_terminal_grid_or_history() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"line one\r\nline two\r\n\x1b[2J\x1b[H$ ");
        let before_lines: Vec<String> = (0..term.rows()).map(|y| term.line_text(y)).collect();
        let before_scrollback = term.scrollback_len();
        let before_cursor = term.cursor();
        let before_visible = term.cursor_visible();
        let mut welcome = WelcomeIdentity::new(false);
        for _ in 0..3 {
            let _ = welcome.present(&term, 80, 24);
        }
        let after_lines: Vec<String> = (0..term.rows()).map(|y| term.line_text(y)).collect();
        assert_eq!(before_lines, after_lines);
        assert_eq!(before_scrollback, term.scrollback_len());
        assert_eq!(before_cursor, term.cursor());
        assert_eq!(before_visible, term.cursor_visible());
    }

    #[test]
    fn production_draw_commands_are_centered_and_bounded() {
        let term = prompt_terminal();
        let mut welcome = WelcomeIdentity::new(false);
        let presentation = welcome.present(&term, 80, 24).expect("identity");
        let viewport = Viewport {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        };
        let commands = presentation.draw_commands(viewport, (10, 20), |text, style| {
            (text.chars().count() as f32 * style.size_px * 0.6).round() as u32
        });
        assert_eq!(commands.len(), presentation.lines.len());
        assert!(commands.iter().any(|c| c.text == SIGNATURE_WORDMARK[0]));
        assert!(commands.iter().any(|c| c.text == SIGNATURE_TAGLINE));
        for command in &commands {
            assert!(command.x >= 0.0);
            assert!(command.baseline > 0.0);
            assert!(command.style.family == UiFontFamily::Monospace);
        }
        let wordmark_x = presentation.rect.col as f32 * 10.0;
        let wordmark_width = presentation.rect.width * 10;
        for (command, line) in commands.iter().zip(&presentation.lines) {
            if line.role == WelcomeLineRole::Wordmark {
                assert_eq!(command.x, wordmark_x);
                assert_eq!(command.max_width, wordmark_width);
            } else {
                assert_eq!(command.max_width, 800);
            }
        }
        let first = &commands[0];
        assert_eq!(first.x, wordmark_x);

        let offset = presentation.draw_commands(
            Viewport {
                x: 5,
                y: 2,
                cols: 80,
                rows: 24,
            },
            (10, 20),
            |text, style| (text.chars().count() as f32 * style.size_px * 0.6).round() as u32,
        );
        assert_eq!(offset[0].x, first.x + 50.0);
        assert_eq!(offset[0].baseline, first.baseline + 40.0);
    }
}
