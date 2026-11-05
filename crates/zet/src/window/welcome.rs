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
/// Shared baseline inside each supporting-copy row, as a fraction of the row.
const COPY_BASELINE: f32 = 0.72;

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

    #[cfg(test)]
    pub const fn contains(self, col: u32, row: u32) -> bool {
        col >= self.col && col < self.right() && row >= self.row && row < self.bottom()
    }
}

/// The semantic ink role of one supporting identity line. Colors stay in the
/// theme so the identity follows the Aperture Instrument palette instead of
/// baking in fixed RGB values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeLineRole {
    Brand,
    Tagline,
    Supporting,
}

impl WelcomeLineRole {
    fn style_and_ink(self, cell_h: u32) -> (UiTextStyle, Rgba) {
        let ch = cell_h.max(1) as f32;
        match self {
            Self::Brand => {
                let size = (ch * 0.72).max(1.0);
                (
                    UiTextStyle::new(size, ch, 700, size * 0.07, UiFontFamily::Monospace),
                    zet_gpu::renderer::PANEL_TEXT,
                )
            }
            Self::Tagline => {
                let size = (ch * 0.62).max(1.0);
                (
                    UiTextStyle::new(size, ch, 500, size * 0.06, UiFontFamily::Monospace),
                    zet_gpu::renderer::PANEL_CYAN,
                )
            }
            Self::Supporting => {
                let size = (ch * 0.54).max(1.0);
                (
                    UiTextStyle::new(size, ch, 500, size * 0.05, UiFontFamily::Monospace),
                    zet_gpu::renderer::PANEL_SECONDARY,
                )
            }
        }
    }
}

fn minimal_style(cell_w: u32, cell_h: u32) -> UiTextStyle {
    let cw = cell_w.max(1) as f32;
    let ch = cell_h.max(1) as f32;
    let size = (cw / 0.6).min(ch * 0.9).max(1.0);
    UiTextStyle::new(size, ch, 600, size * 0.06, UiFontFamily::Monospace)
}

/// One supporting identity line, positioned relative to the terminal viewport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeLine {
    pub row: u32,
    pub text: &'static str,
    pub role: WelcomeLineRole,
}

/// A resolved identity presentation. `rect` is the exact reserved cell
/// region; `wordmark_rows` is the approved character grid, and `copy_lines`
/// are the supporting native text lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomePresentation {
    pub size: SignatureIdentitySize,
    pub rect: WelcomeRect,
    pub wordmark_rows: &'static [&'static str],
    pub copy_lines: Vec<WelcomeLine>,
    pub minimal_text: Option<&'static str>,
    pub viewport_cols: u32,
    pub viewport_rows: u32,
}

/// One pixel-snapped native wordmark rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordmarkRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
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
    /// Top of the reserved wordmark block, pixel-centred in the viewport.
    fn wordmark_origin(&self, viewport: Viewport, cell_px: (u32, u32)) -> (u32, u32) {
        let cw = cell_px.0.max(1);
        let ch = cell_px.1.max(1);
        let left = viewport.x.saturating_mul(cw);
        let width = viewport.cols.saturating_mul(cw);
        let art_width = self.rect.width.saturating_mul(cw);
        let x = left.saturating_add(width.saturating_sub(art_width) / 2);
        let y = viewport.y.saturating_add(self.rect.row).saturating_mul(ch);
        (x, y)
    }

    /// Build the supporting copy and minimal-mark commands used by the
    /// application. The caller supplies the production glyph measurement
    /// function, so tests can assert placement without a display.
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
        let mut commands = Vec::new();
        for line in &self.copy_lines {
            if line.row >= viewport.rows {
                continue;
            }
            let (style, ink) = line.role.style_and_ink(ch);
            let measured = measure(line.text, style).min(width);
            commands.push(WelcomeDrawCommand {
                x: left + (width.saturating_sub(measured) as f32 / 2.0),
                baseline: (viewport.y.saturating_add(line.row) as f32 * ch as f32)
                    + ch as f32 * COPY_BASELINE,
                text: line.text,
                style,
                ink,
                max_width: width,
            });
        }
        if let Some(text) = self.minimal_text {
            if self.rect.row < viewport.rows {
                let style = minimal_style(cw, ch);
                let measured = measure(text, style).min(width);
                commands.push(WelcomeDrawCommand {
                    x: left + (width.saturating_sub(measured) as f32 / 2.0),
                    baseline: (viewport.y.saturating_add(self.rect.row) as f32 * ch as f32)
                        + ch as f32 * COPY_BASELINE,
                    text,
                    style,
                    ink: zet_gpu::renderer::PANEL_CYAN,
                    max_width: width,
                });
            }
        }
        commands
    }
}

/// Convert the approved character grid into pixel-snapped native rectangles.
///
/// The font's double-line box glyphs are inset inside their own cells while
/// `█` fills a cell completely; that is what made the thin accents wander off
/// the block edges. Here horizontal box runs become two precise rules attached
/// to the top of their row, with one shared thickness/gap pair. The vertical
/// `║` accents are omitted: the solid stems already carry the silhouette and
/// the remaining rules are the deliberate instrument detail. The character
/// grid and letterform silhouette are unchanged.
pub fn wordmark_rects(
    rows: &[&str],
    origin_x: u32,
    origin_y: u32,
    cell_px: (u32, u32),
) -> Vec<WordmarkRect> {
    let cw = cell_px.0.max(1);
    let ch = cell_px.1.max(1);
    let thickness = ((ch as f32 * 0.075).round() as u32).max(1);
    let gap = ((ch as f32 * 0.060).round() as u32).max(1);
    let mut rects = Vec::new();
    let mut push = |x: u32, y: u32, width: u32, height: u32| {
        if width > 0 && height > 0 {
            rects.push(WordmarkRect {
                x,
                y,
                width,
                height,
            });
        }
    };

    for (row, line) in rows.iter().enumerate() {
        let y = origin_y.saturating_add(row as u32 * ch);
        let chars: Vec<char> = line.chars().collect();
        let mut col = 0usize;
        while col < chars.len() {
            let c = chars[col];
            let x = origin_x.saturating_add(col as u32 * cw);
            match c {
                '█' | '▄' => {
                    let start = col;
                    while col < chars.len() && chars[col] == c {
                        col += 1;
                    }
                    let cells = (col - start) as u32;
                    if c == '█' {
                        push(x, y, cells * cw, ch);
                    } else {
                        let top = ch / 2;
                        push(x, y.saturating_add(top), cells * cw, ch - top);
                    }
                }
                '╔' | '╚' | '╗' | '╝' | '═' => {
                    let start = col;
                    while col < chars.len() && matches!(chars[col], '╔' | '╚' | '╗' | '╝' | '═')
                    {
                        col += 1;
                    }
                    let width = (col - start) as u32 * cw;
                    push(x, y, width, thickness);
                    push(x, y.saturating_add(thickness + gap), width, thickness);
                }
                // `║` is a vertical accent only; the solid stem already
                // defines the edge, so it is intentionally not painted.
                _ => col += 1,
            }
        }
    }
    rects
}

/// Draw the identity through the native presentation layer. The wordmark is
/// pixel-snapped geometry; supporting copy uses the native UI text stream.
/// Neither path touches the terminal renderer or PTY cells.
pub fn draw(r: &mut Renderer, viewport: Viewport, presentation: &WelcomePresentation) {
    let cell_px = r.cell_px();
    if !presentation.wordmark_rows.is_empty() {
        let (x, y) = presentation.wordmark_origin(viewport, cell_px);
        for rect in wordmark_rects(presentation.wordmark_rows, x, y, cell_px) {
            r.draw_rect_px(
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                zet_gpu::renderer::PANEL_CYAN,
            );
        }
    }
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

fn build_copy_lines(size: SignatureIdentitySize, top: usize) -> Vec<WelcomeLine> {
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
            push(&mut lines, 9, SIGNATURE_BRAND_COPY, WelcomeLineRole::Brand);
        }
        SignatureIdentitySize::Minimal | SignatureIdentitySize::Omitted => {}
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
        let wordmark_rows = match size {
            SignatureIdentitySize::Full | SignatureIdentitySize::Compact => {
                signature_wordmark_lines(size)
            }
            SignatureIdentitySize::Minimal | SignatureIdentitySize::Omitted => &[],
        };
        let minimal_text = match size {
            SignatureIdentitySize::Minimal => Some(SIGNATURE_MINIMAL_TEXT),
            _ => None,
        };
        return Some(WelcomePresentation {
            size,
            rect: WelcomeRect {
                col,
                row: top as u32,
                width,
                height: height as u32,
            },
            wordmark_rows,
            copy_lines: build_copy_lines(size, top),
            minimal_text,
            viewport_cols,
            viewport_rows,
        });
    }
    None
}

/// True when terminal content, the cursor, scrolling, or a full-screen app
/// has reached a previously reserved identity row.
pub fn rect_conflicts(term: &Terminal, rect: &WelcomeRect) -> bool {
    if term.alt_active() || term.view_offset() != 0 {
        return true;
    }
    if rect.right() > term.cols() as u32 || rect.bottom() > term.rows() as u32 {
        return true;
    }
    let (_, cursor_row) = term.cursor();
    if rect.row <= cursor_row as u32 && (cursor_row as u32) < rect.bottom() {
        return true;
    }
    (rect.row as usize..rect.bottom() as usize)
        .any(|y| (0..term.cols()).any(|x| term.visible_cell(x, y).is_some_and(cell_is_content)))
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
        assert_eq!(presentation.wordmark_rows.len(), 8);
        assert_eq!(presentation.copy_lines.len(), 3);
        assert_eq!(presentation.wordmark_rows[0], SIGNATURE_WORDMARK[0]);
        assert_eq!(presentation.copy_lines[0].text, SIGNATURE_BRAND_COPY);
        assert!(presentation.minimal_text.is_none());
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
        assert!(presentation.wordmark_rows.is_empty());
        assert!(presentation.copy_lines.is_empty());
        assert_eq!(presentation.minimal_text, Some(SIGNATURE_MINIMAL_TEXT));
        assert_eq!(presentation.rect.height, 1);
        let commands = presentation.draw_commands(
            Viewport {
                x: 0,
                y: 0,
                cols: 20,
                rows: 24,
            },
            (10, 20),
            |text, style| (text.chars().count() as f32 * style.size_px * 0.6).round() as u32,
        );
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].text, SIGNATURE_MINIMAL_TEXT);
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
        assert_eq!(commands.len(), presentation.copy_lines.len());
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].text, SIGNATURE_BRAND_COPY);
        assert_eq!(commands[1].text, SIGNATURE_TAGLINE);
        assert_eq!(commands[2].text, SIGNATURE_SUPPORTING_IDENTITY);
        for command in &commands {
            assert!(command.x >= 0.0);
            assert!(command.baseline > 0.0);
            assert_eq!(command.max_width, 800);
            assert_eq!(command.style.family, UiFontFamily::Monospace);
        }
        assert!(commands[0].style.size_px > commands[1].style.size_px);
        assert!(commands[1].style.size_px > commands[2].style.size_px);
        assert!((commands[1].baseline - commands[0].baseline - 20.0).abs() < 0.01);
        assert!((commands[2].baseline - commands[1].baseline - 20.0).abs() < 0.01);
        for command in &commands {
            let measured =
                (command.text.chars().count() as f32 * command.style.size_px * 0.6) as u32;
            let expected = (800u32.saturating_sub(measured)) as f32 / 2.0;
            assert!((command.x - expected).abs() <= 1.0);
        }

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
        assert_eq!(offset[0].x, commands[0].x + 50.0);
        assert_eq!(offset[0].baseline, commands[0].baseline + 40.0);
    }

    #[test]
    fn wordmark_geometry_snaps_blocks_and_rules_to_cells() {
        let cell = (10u32, 20u32);
        let rects = wordmark_rects(SIGNATURE_WORDMARK, 100, 200, cell);
        assert!(!rects.is_empty());
        for rect in &rects {
            assert!(rect.width > 0 && rect.height > 0);
            assert!(rect.x >= 100);
            assert!(rect.y >= 200);
            assert!(rect.x + rect.width <= 100 + WORDMARK_COLS * cell.0);
            assert!(rect.y + rect.height <= 200 + SIGNATURE_WORDMARK.len() as u32 * cell.1);
        }

        // The first row's leading full-block run is one merged rectangle, so
        // no cell seams can appear inside the letterform.
        assert!(rects.contains(&WordmarkRect {
            x: 120,
            y: 200,
            width: 12 * cell.0,
            height: cell.1,
        }));
        // Row two starts with `╚══════`: the seven-cell horizontal run is
        // merged into one crisp top rule and one inset rule.
        let thickness = ((cell.1 as f32 * 0.075).round() as u32).max(1);
        let gap = ((cell.1 as f32 * 0.060).round() as u32).max(1);
        assert!(rects.contains(&WordmarkRect {
            x: 120,
            y: 220,
            width: 7 * cell.0,
            height: thickness,
        }));
        assert!(rects.contains(&WordmarkRect {
            x: 120,
            y: 220 + thickness + gap,
            width: 7 * cell.0,
            height: thickness,
        }));
        // `▄` is an exact lower-half cell, not a font-shaped approximation.
        assert!(rects.contains(&WordmarkRect {
            x: 170,
            y: 250,
            width: cell.0,
            height: cell.1 / 2,
        }));
    }

    #[test]
    fn wordmark_rules_share_one_consistent_stroke_width() {
        let cell = (10u32, 20u32);
        let rects = wordmark_rects(SIGNATURE_WORDMARK, 0, 0, cell);
        let thickness = ((cell.1 as f32 * 0.075).round() as u32).max(1);
        let stroked: Vec<_> = rects
            .iter()
            .filter(|rect| rect.height == thickness)
            .collect();
        assert!(!stroked.is_empty());
        for rect in stroked {
            assert!(rect.width >= thickness);
            assert_eq!(rect.height, thickness);
        }
    }

    #[test]
    fn wordmark_omits_floating_vertical_outlines() {
        let cell = (10u32, 20u32);
        let rects = wordmark_rects(SIGNATURE_WORDMARK, 0, 0, cell);
        let thickness = ((cell.1 as f32 * 0.075).round() as u32).max(1);
        // R1's visible defect was a one-sided vertical outline floating
        // beside a stem. The refined grid keeps only horizontal accents.
        assert!(!rects
            .iter()
            .any(|rect| rect.width == thickness && rect.height == cell.1));
    }

    fn centered(viewport_width: u32, measured: u32) -> f32 {
        (viewport_width.saturating_sub(measured)) as f32 / 2.0
    }

    #[test]
    fn wordmark_origin_is_pixel_centered() {
        let term = prompt_terminal();
        let presentation = plan(&term, 79, 24).expect("identity");
        let viewport = Viewport {
            x: 0,
            y: 0,
            cols: 79,
            rows: 24,
        };
        let (x, y) = presentation.wordmark_origin(viewport, (10, 20));
        let art_width = presentation.rect.width * 10;
        assert_eq!(x, (79 * 10 - art_width) / 2);
        assert_eq!(y, presentation.rect.row * 20);
        assert_eq!(x as f32, centered(79 * 10, art_width));
    }
}
