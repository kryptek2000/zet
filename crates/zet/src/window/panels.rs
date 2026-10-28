//! Mission Rail data model (Slice 4B).
//!
//! The rail is a read-only view over real PTY sessions and observed
//! commands. Every row carries a provenance so the UI never implies more
//! certainty than its source supports. Unknown stays unknown (`—`).

use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where a displayed fact came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    Observed,
    Inferred,
    Unknown,
}

/// One row in a panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelRow {
    pub kind: RowKind,
    pub label: String,
    pub value: String,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    SectionHeader,
    Session,
    Command,
    Status,
    Empty,
}

/// A real open session as shown in the rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub index: usize,
    pub cwd_tail: String,
    /// Display path with the user's home directory shortened to `~`.
    /// Empty when the process cwd is unavailable.
    pub cwd: String,
    pub shell: String,
    pub state: String,
    pub exit: String,
    pub provenance: Provenance,
}

/// A real recorded command as shown in the rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRow {
    pub id: i64,
    pub command: String,
    pub exit: String,
    pub duration: String,
    pub cwd: String,
    pub git_branch: String,
    pub provenance: Provenance,
}

/// Immutable snapshot rendered by the rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailSnapshot {
    pub mode: String,
    pub active: usize,
    pub sessions: Vec<SessionRow>,
    pub commands: Vec<CommandRow>,
    pub footer: String,
    pub footer_provenance: Provenance,
}

/// Cells consumed by one Unicode scalar in a rail row.
///
/// The rail paints one glyph per scalar, so every printable scalar needs
/// at least one cell. East Asian wide and emoji scalars are budgeted at
/// two cells so the same budget is also safe on a double-width terminal.
/// Control scalars do not paint and consume no cell.
pub fn char_cells(c: char) -> usize {
    if c.is_control() {
        0
    } else if is_wide_scalar(c) {
        2
    } else {
        1
    }
}

/// Terminal cells occupied by `text` under the rail's width model.
pub fn text_cells(text: &str) -> usize {
    text.chars().map(char_cells).sum()
}

fn is_wide_scalar(c: char) -> bool {
    matches!(
        c as u32,
        0x1100..=0x115F
            | 0x2329..=0x232A
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F680..=0x1F6FF
            | 0x1F900..=0x1F9FF
            | 0x1FA70..=0x1FAFF
            | 0x20000..=0x3FFFD
    )
}

/// Replace control scalars with spaces so a row always paints as one
/// visible line and the width model matches the painted cells.
fn single_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Truncate from the right to `max_cells`, using a trailing ellipsis that
/// counts against the budget. Never splits a scalar.
pub fn truncate_to_cells(text: &str, max_cells: usize) -> String {
    if text_cells(text) <= max_cells {
        return text.to_string();
    }
    if max_cells == 0 {
        return String::new();
    }
    let budget = max_cells - 1;
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cells = char_cells(c);
        if used + cells > budget {
            break;
        }
        out.push(c);
        used += cells;
    }
    if !out.ends_with('…') {
        out.push('…');
    }
    out
}

/// Truncate from the left to `max_cells`, keeping the rightmost (most
/// specific) part of a label or path behind a leading ellipsis.
pub fn truncate_left_to_cells(text: &str, max_cells: usize) -> String {
    if text_cells(text) <= max_cells {
        return text.to_string();
    }
    if max_cells == 0 {
        return String::new();
    }
    let budget = max_cells - 1;
    let mut kept: Vec<char> = Vec::new();
    let mut used = 0;
    for c in text.chars().rev() {
        let cells = char_cells(c);
        if used + cells > budget {
            break;
        }
        kept.push(c);
        used += cells;
    }
    let mut out = String::new();
    if kept.last() != Some(&'…') {
        out.push('…');
    }
    out.extend(kept.into_iter().rev());
    out
}

/// A command row in exactly `cols` cells or fewer:
/// `<command> <exit> <duration>`.
///
/// The exit and duration fields are reserved before the command is given
/// the remainder, so a long command can never clip a duration unit. If the
/// rail is too narrow for the exit code, the complete duration survives;
/// if even the complete duration cannot fit, only an ellipsis plus its
/// unit is shown — never a bare number that could be misread.
pub fn format_command_row(command: &str, exit: &str, duration: &str, cols: u32) -> String {
    let cols = cols as usize;
    if cols == 0 {
        return String::new();
    }
    let command = single_line(command);
    let exit = single_line(exit);
    let duration = single_line(duration);

    let duration_cells = text_cells(&duration);
    if duration.is_empty() || duration_cells > cols {
        return compact_duration(&duration, cols);
    }

    let exit_cells = text_cells(&exit);
    let right = if !exit.is_empty() && exit_cells + 1 + duration_cells <= cols {
        format!("{exit} {duration}")
    } else {
        duration
    };
    let right_cells = text_cells(&right);
    if right_cells >= cols {
        return right;
    }
    let command_budget = cols - right_cells - 1;
    let command = truncate_to_cells(&command, command_budget);
    if command.is_empty() {
        right
    } else {
        format!("{command} {right}")
    }
}

fn compact_duration(duration: &str, cols: usize) -> String {
    if cols == 0 {
        return String::new();
    }
    if duration.is_empty() {
        return "…".to_string();
    }
    let unit: String = duration
        .chars()
        .rev()
        .take_while(|c| c.is_alphabetic())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let suffix = if unit.is_empty() {
        "…".to_string()
    } else {
        format!("…{unit}")
    };
    if text_cells(&suffix) <= cols {
        suffix
    } else {
        "…".to_string()
    }
}

/// Shorten the user's home directory to `~` for rail display. Paths
/// outside home, or a missing home value, pass through unchanged.
pub fn shorten_home(path: &str, home: &str) -> String {
    let path = single_line(path);
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return path;
    }
    if path == home {
        return "~".to_string();
    }
    match path.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path,
    }
}

/// Split a session detail row into a state label and a `· path` suffix.
/// The caller draws the state in its semantic ink and the path dim; the
/// two strings together fit within `cols` when the state starts one cell
/// in from the rail edge.
pub fn format_session_detail(state: &str, cwd: &str, cols: u32) -> (String, String) {
    let cols = cols as usize;
    let state = truncate_to_cells(&single_line(state), cols);
    let state_cells = text_cells(&state);
    if cols == 0 || state_cells + 2 >= cols {
        return (state, String::new());
    }
    let cwd = single_line(cwd);
    if cwd.is_empty() {
        return (state, String::new());
    }
    let available = cols - state_cells - 2;
    if available <= 2 {
        return (state, String::new());
    }
    let path_budget = available - 2;
    (
        state,
        format!("· {}", truncate_left_to_cells(&cwd, path_budget)),
    )
}

/// Footer row in exactly `cols` cells or fewer. The path keeps its tail
/// so the current directory stays identifiable in a narrow rail.
pub fn format_footer(cwd: &str, cols: u32) -> String {
    let cols = cols as usize;
    if cols == 0 {
        return String::new();
    }
    let cwd = single_line(cwd);
    let prefix = "cwd ";
    let prefix_cells = text_cells(prefix);
    if cols <= prefix_cells || cwd.is_empty() {
        return truncate_to_cells("cwd", cols);
    }
    let budget = cols - prefix_cells;
    format!("{prefix}{}", truncate_left_to_cells(&cwd, budget))
}

/// Dirty-flagged cache for panel snapshots. The rail is rebuilt at most
/// once per frame and only when something observable changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeCache {
    pub snapshot: Option<Arc<RailSnapshot>>,
    pub dirty: bool,
    pub last_active: usize,
    pub last_layout_rail: bool,
    pub last_blocks: usize,
    pub last_phase: Option<String>,
    pub last_cwd: String,
    pub last_tab_count: usize,
    pub last_active_dead: bool,
    pub last_refresh: Option<Instant>,
    pub poll_interval: Duration,
}

impl ChromeCache {
    pub fn new() -> Self {
        Self {
            snapshot: None,
            dirty: true,
            last_active: usize::MAX,
            last_layout_rail: false,
            last_blocks: usize::MAX,
            last_phase: None,
            last_cwd: String::new(),
            last_tab_count: usize::MAX,
            last_active_dead: true,
            last_refresh: None,
            poll_interval: Duration::from_millis(250),
        }
    }

    pub fn snapshot(&self) -> Option<&RailSnapshot> {
        self.snapshot.as_deref()
    }

    pub fn snapshot_handle(&self) -> Option<Arc<RailSnapshot>> {
        self.snapshot.clone()
    }

    pub fn poll_due(&self, now: Instant) -> bool {
        self.last_refresh
            .is_none_or(|last| now.duration_since(last) >= self.poll_interval)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn cheap_changed(
        &self,
        active: usize,
        layout_rail: bool,
        phase: Option<zet_pty::shell::Phase>,
        cwd: &str,
        tab_count: usize,
        active_dead: bool,
    ) -> bool {
        let phase = phase.map(|phase| format!("{phase:?}"));
        self.dirty
            || self.last_active != active
            || self.last_layout_rail != layout_rail
            || self.last_tab_count != tab_count
            || self.last_active_dead != active_dead
            || self.last_phase != phase
            || self.last_cwd != cwd
    }

    #[allow(clippy::too_many_arguments)]
    pub fn needs_refresh(
        &self,
        active: usize,
        layout_rail: bool,
        phase: Option<zet_pty::shell::Phase>,
        cwd: &str,
        tab_count: usize,
        active_dead: bool,
        now: Instant,
    ) -> bool {
        let phase = phase.map(|phase| format!("{phase:?}"));
        if self.dirty
            || self.last_active != active
            || self.last_layout_rail != layout_rail
            || self.last_tab_count != tab_count
            || self.last_active_dead != active_dead
            || self.last_phase != phase
            || self.last_cwd != cwd
        {
            return true;
        }
        self.last_refresh
            .is_none_or(|last| now.duration_since(last) >= self.poll_interval)
    }

    pub fn mark_refreshed(&mut self, now: Instant) {
        self.last_refresh = Some(now);
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn commit_refresh(
        &mut self,
        active: usize,
        layout_rail: bool,
        block_count: usize,
        phase: Option<zet_pty::shell::Phase>,
        cwd: String,
        tab_count: usize,
        active_dead: bool,
        snapshot: RailSnapshot,
        now: Instant,
    ) {
        self.snapshot = Some(Arc::new(snapshot));
        self.last_active = active;
        self.last_layout_rail = layout_rail;
        self.last_blocks = block_count;
        self.last_phase = phase.map(|phase| format!("{phase:?}"));
        self.last_cwd = cwd;
        self.last_tab_count = tab_count;
        self.last_active_dead = active_dead;
        self.last_refresh = Some(now);
        self.dirty = false;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn refresh_if_changed(
        &mut self,
        active: usize,
        layout_rail: bool,
        block_count: usize,
        phase: Option<zet_pty::shell::Phase>,
        cwd: String,
        tab_count: usize,
        active_dead: bool,
        build: impl FnOnce() -> RailSnapshot,
    ) {
        let phase = phase.map(|phase| format!("{phase:?}"));
        if self.dirty
            || self.last_active != active
            || self.last_layout_rail != layout_rail
            || self.last_blocks != block_count
            || self.last_phase != phase
            || self.last_cwd != cwd
            || self.last_tab_count != tab_count
            || self.last_active_dead != active_dead
        {
            self.snapshot = Some(Arc::new(build()));
            self.last_active = active;
            self.last_layout_rail = layout_rail;
            self.last_blocks = block_count;
            self.last_phase = phase;
            self.last_cwd = cwd;
            self.last_tab_count = tab_count;
            self.last_active_dead = active_dead;
            self.dirty = false;
        }
    }
}

impl Default for ChromeCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Displayed rail row to session index. Header rows and non-session rows
/// never select a session.
/// One resolved rail row. The footer, when present, always occupies the
/// last row and never overlaps a session or command row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailRow {
    Header,
    Mode,
    SessionsHeader,
    Session(usize),
    SessionDetail(usize),
    CommandsHeader,
    Command(usize),
    Footer,
    Empty,
}

/// Rows used by one session card: a title row and a path/state row.
pub const SESSION_CARD_ROWS: usize = 2;
/// Expanded rails leave one blank row between cards. Compact rails spend
/// every row on a real session instead.
pub const SESSION_GAP_ROWS: usize = 1;

/// Deterministic rail row layout shared by rendering and hit testing.
///
/// Expanded rails get a brand row, a mode row, a spacer, the sessions
/// header and two-row session cards. Short rails drop the optional mode
/// row and spacer so at least two real sessions remain visible at the
/// 10-row terminal minimum. Commands are allocated the rows left over
/// after every visible session; no row is ever invented for a session
/// that does not exist.
pub fn rail_rows(viewport_rows: u32, session_count: usize, command_count: usize) -> Vec<RailRow> {
    let mut rows = vec![RailRow::Empty; viewport_rows as usize];
    if viewport_rows == 0 {
        return rows;
    }
    let footer_row = viewport_rows as usize - 1;
    rows[0] = RailRow::Header;

    let compact = viewport_rows < 16;
    let session_start = if compact {
        if footer_row > 1 {
            rows[1] = RailRow::SessionsHeader;
        }
        2usize
    } else {
        if footer_row > 1 {
            rows[1] = RailRow::Mode;
        }
        if footer_row > 2 {
            rows[2] = RailRow::Empty;
        }
        if footer_row > 3 {
            rows[3] = RailRow::SessionsHeader;
        }
        4usize
    };

    let mut cursor = session_start;
    let mut shown = 0usize;
    while shown < session_count && cursor + SESSION_CARD_ROWS <= footer_row {
        rows[cursor] = RailRow::Session(shown);
        rows[cursor + 1] = RailRow::SessionDetail(shown);
        cursor += SESSION_CARD_ROWS;
        shown += 1;
        if !compact
            && shown < session_count
            && cursor + SESSION_GAP_ROWS + SESSION_CARD_ROWS <= footer_row
        {
            cursor += SESSION_GAP_ROWS;
        }
    }

    if command_count > 0 && cursor + 2 < footer_row {
        rows[cursor] = RailRow::Empty;
        cursor += 1;
        rows[cursor] = RailRow::CommandsHeader;
        cursor += 1;
        let command_capacity = footer_row.saturating_sub(cursor);
        let visible_commands = command_count.min(command_capacity);
        for i in 0..visible_commands {
            rows[cursor + i] = RailRow::Command(i);
        }
    }

    rows[footer_row] = RailRow::Footer;
    rows
}

/// Session index for a clicked rail row, if any. Both rows of a session
/// card select the same session.
pub fn rail_session_at_row(row: u32, session_count: usize, viewport_rows: u32) -> Option<usize> {
    let rows = rail_rows(viewport_rows, session_count, 0);
    match rows.get(row as usize) {
        Some(RailRow::Session(index)) | Some(RailRow::SessionDetail(index)) => Some(*index),
        _ => None,
    }
}

/// Backwards-compatible alias.
pub fn rail_row_target(row: u32, session_count: usize, viewport_rows: u32) -> Option<usize> {
    rail_session_at_row(row, session_count, viewport_rows)
}

/// Backwards-compatible alias.
pub fn rail_row_layout(
    viewport_rows: u32,
    session_count: usize,
    command_count: usize,
) -> Vec<RailRow> {
    rail_rows(viewport_rows, session_count, command_count)
}

#[cfg(test)]
mod rail_row_tests {
    use super::{rail_rows, rail_session_at_row, RailRow};

    #[test]
    fn one_session_has_two_rows_and_footer_last() {
        let rows = rail_rows(8, 1, 1);
        assert_eq!(rows[0], RailRow::Header);
        assert_eq!(rows[1], RailRow::SessionsHeader);
        assert_eq!(rows[2], RailRow::Session(0));
        assert_eq!(rows[3], RailRow::SessionDetail(0));
        assert_eq!(rows[7], RailRow::Footer);
        assert_eq!(rail_session_at_row(2, 1, 8), Some(0));
        assert_eq!(rail_session_at_row(3, 1, 8), Some(0));
        assert_eq!(rail_session_at_row(7, 1, 8), None);
    }

    #[test]
    fn two_sessions_do_not_touch_footer() {
        let rows = rail_rows(8, 2, 1);
        assert_eq!(rows[2], RailRow::Session(0));
        assert_eq!(rows[3], RailRow::SessionDetail(0));
        assert_eq!(rows[4], RailRow::Session(1));
        assert_eq!(rows[5], RailRow::SessionDetail(1));
        assert_eq!(rows[7], RailRow::Footer);
        assert_eq!(rail_session_at_row(4, 2, 8), Some(1));
        assert_eq!(rail_session_at_row(5, 2, 8), Some(1));
    }

    #[test]
    fn sessions_are_bounded_by_two_row_cards_and_footer() {
        let rows = rail_rows(12, 7, 2);
        let sessions: Vec<_> = rows
            .iter()
            .filter(|r| matches!(r, RailRow::Session(_)))
            .collect();
        // Compact rail: header + sessions header, then four two-row cards.
        assert_eq!(sessions.len(), 4);
        assert_eq!(rows[2], RailRow::Session(0));
        assert_eq!(rows[8], RailRow::Session(3));
        assert_eq!(rows[9], RailRow::SessionDetail(3));
        assert_eq!(rows[10], RailRow::Empty);
        assert_eq!(rows[11], RailRow::Footer);
    }

    #[test]
    fn expanded_rail_shows_mode_and_observed_commands() {
        let rows = rail_rows(20, 2, 3);
        assert_eq!(rows[0], RailRow::Header);
        assert_eq!(rows[1], RailRow::Mode);
        assert_eq!(rows[2], RailRow::Empty);
        assert_eq!(rows[3], RailRow::SessionsHeader);
        assert_eq!(rows[4], RailRow::Session(0));
        assert_eq!(rows[5], RailRow::SessionDetail(0));
        assert_eq!(rows[6], RailRow::Empty);
        assert_eq!(rows[7], RailRow::Session(1));
        assert_eq!(rows[8], RailRow::SessionDetail(1));
        assert!(rows.contains(&RailRow::CommandsHeader));
        assert_eq!(
            rows.iter()
                .filter(|r| matches!(r, RailRow::Command(_)))
                .count(),
            3
        );
        assert_eq!(rows[19], RailRow::Footer);
    }

    #[test]
    fn footer_is_never_overwritten_and_cards_stay_paired() {
        for rows in 0u32..=40 {
            for sessions in 0usize..=8 {
                for commands in 0usize..=8 {
                    let layout = rail_rows(rows, sessions, commands);
                    if rows == 0 {
                        assert!(layout.is_empty());
                        continue;
                    }
                    assert_eq!(layout[rows as usize - 1], RailRow::Footer);
                    for (i, row) in layout.iter().enumerate() {
                        if i != rows as usize - 1 {
                            assert_ne!(*row, RailRow::Footer);
                        }
                        if let RailRow::Session(index) = row {
                            assert_eq!(
                                layout.get(i + 1),
                                Some(&RailRow::SessionDetail(*index)),
                                "session {index} at row {i} has no detail row (rows={rows})"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn narrow_viewport_has_no_session_under_footer() {
        for rows in [0u32, 1, 2, 3, 4] {
            let layout = rail_rows(rows, 3, 1);
            assert_eq!(layout.len(), rows as usize);
            if rows > 0 {
                assert_eq!(layout[rows as usize - 1], RailRow::Footer);
            }
            assert!(
                layout
                    .iter()
                    .filter(|r| matches!(r, RailRow::Session(_)))
                    .count()
                    <= rows as usize
            );
        }
    }

    #[test]
    fn footer_never_activates_a_session() {
        for rows in 0u32..=20 {
            assert_eq!(rail_session_at_row(rows.saturating_sub(1), 3, rows), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_footer_is_not_invented() {
        let snap = RailSnapshot {
            mode: "Terminal Focus".to_string(),
            active: 0,
            sessions: Vec::new(),
            commands: Vec::new(),
            footer: "—".to_string(),
            footer_provenance: Provenance::Unknown,
        };
        assert_eq!(snap.footer, "—");
        assert_eq!(snap.footer_provenance, Provenance::Unknown);
    }

    #[test]
    fn home_is_shortened_without_touching_other_paths() {
        assert_eq!(shorten_home("/home/bandit", "/home/bandit"), "~");
        assert_eq!(
            shorten_home("/home/bandit/zet/crates/api", "/home/bandit"),
            "~/zet/crates/api"
        );
        assert_eq!(shorten_home("/tmp/zet", "/home/bandit"), "/tmp/zet");
        assert_eq!(
            shorten_home("/home/bandit2", "/home/bandit"),
            "/home/bandit2"
        );
        assert_eq!(shorten_home("/home/bandit", ""), "/home/bandit");
    }

    #[test]
    fn session_detail_fits_state_and_path_in_the_rail() {
        for cols in [12u32, 18, 23, 40] {
            let (state, path) = format_session_detail("running", "~/zet/crates/api", cols);
            let exclusive = text_cells(&state) + 1 + text_cells(&path);
            assert!(
                exclusive <= cols as usize,
                "{state:?} + {path:?} = {exclusive} > {cols}"
            );
        }
        let (state, path) = format_session_detail("running", "~/zet/crates/api", 23);
        assert_eq!(state, "running");
        assert!(path.starts_with("· "), "{path:?}");
        assert!(path.ends_with("crates/api"), "{path:?}");
    }

    #[test]
    fn session_detail_degrades_without_inventing_a_path() {
        let (state, path) = format_session_detail("exited", "", 23);
        assert_eq!(state, "exited");
        assert!(path.is_empty());
        let (state, path) = format_session_detail("running", "~/zet", 4);
        assert_eq!(text_cells(&state), 4);
        assert!(path.is_empty());
    }

    fn assert_fits(line: &str, cols: u32) {
        let cells = text_cells(line);
        assert!(
            cells <= cols as usize,
            "{line:?} uses {cells} cells, rail has {cols}"
        );
        // The rail paints one glyph per scalar, so the painted glyph count
        // must fit too, even when wide scalars were budgeted at two cells.
        assert!(
            line.chars().count() <= cols as usize,
            "{line:?} paints {} glyphs, rail has {cols}",
            line.chars().count()
        );
    }

    #[test]
    fn cell_width_counts_scalars_not_bytes() {
        assert_eq!(text_cells("abc"), 3);
        assert_eq!(text_cells("café"), 4);
        assert_eq!(text_cells("日本語"), 6);
        assert_eq!(text_cells("🙂"), 2);
        assert_eq!(text_cells("a\nb"), 2); // control scalars do not paint
        assert_eq!(chars_in("日本語"), 3);
    }

    fn chars_in(text: &str) -> usize {
        text.chars().count()
    }

    #[test]
    fn truncation_keeps_scalar_boundaries() {
        assert_eq!(truncate_to_cells("abcdef", 3), "ab…");
        assert_eq!(truncate_to_cells("abc", 3), "abc");
        assert_eq!(truncate_to_cells("abcdef", 0), "");
        assert_eq!(truncate_to_cells("abcdef", 1), "…");
        assert_eq!(truncate_left_to_cells("abcdef", 3), "…ef");
        assert_eq!(truncate_left_to_cells("abc", 3), "abc");
        assert_eq!(truncate_left_to_cells("abcdef", 0), "");
        assert_eq!(truncate_left_to_cells("abcdef", 1), "…");
        // A wide scalar never straddles the budget: it is dropped whole.
        assert_eq!(truncate_to_cells("日本語", 4), "日…");
        assert_eq!(truncate_to_cells("日本語", 3), "日…");
    }

    #[test]
    fn short_command_rows_keep_their_observed_values() {
        assert_eq!(format_command_row("pwd", "0", "6ms", 24), "pwd 0 6ms");
        assert_eq!(format_command_row("ls -la", "0", "5ms", 24), "ls -la 0 5ms");
        assert_eq!(
            format_command_row("cargo test", "127", "812ms", 24),
            "cargo test 127 812ms"
        );
    }

    #[test]
    fn duration_unit_survives_rail_command_truncation() {
        // The confirmed native defect: a long command consumed the whole
        // 24-column rail and `14ms` was painted as `14`.
        let row = format_command_row("echo hello-from-zet-audit", "0", "14ms", 24);
        assert_eq!(row, "echo hello-from-… 0 14ms");
        assert!(row.ends_with(" 0 14ms"));
        assert!(!row.ends_with("14"));
        assert!(row.contains('…'), "truncation must be visible: {row:?}");
        assert_fits(&row, 24);
    }

    #[test]
    fn long_command_never_clips_duration_or_exit_code() {
        for (command, exit, duration) in [
            ("echo hello-from-zet-audit", "0", "14ms"),
            ("cargo build --release --all-targets", "127", "123456ms"),
            (
                "git push origin feat/p2a-slice-4b-mission-rail",
                "-1",
                "9ms",
            ),
            ("rg --files-with-matches needle .", "255", "1000000ms"),
        ] {
            let row = format_command_row(command, exit, duration, 24);
            assert!(
                row.ends_with(&format!("{exit} {duration}")),
                "row {row:?} lost exit/duration for {command:?}"
            );
            assert!(row.contains('…'), "row {row:?} hides truncation");
            assert_fits(&row, 24);
        }
    }

    #[test]
    fn unknown_values_stay_unknown_and_bounded() {
        assert_eq!(format_command_row("make", "—", "—", 24), "make — —");
        let row = format_command_row("make --a-very-long-target-name", "—", "—", 24);
        assert!(row.ends_with("— —"));
        assert_fits(&row, 24);
    }

    #[test]
    fn narrow_rails_never_lose_duration_units() {
        for cols in 0..=24u32 {
            let row = format_command_row("echo hello-from-zet-audit", "0", "14ms", cols);
            assert_fits(&row, cols);
            if cols >= 4 {
                assert!(
                    row.ends_with("14ms"),
                    "cols={cols}: duration unit lost in {row:?}"
                );
            } else {
                assert!(
                    !row.chars().any(|c| c.is_ascii_digit()),
                    "cols={cols}: bare number {row:?}"
                );
            }
            if cols >= 6 {
                assert!(row.contains("0 14ms"), "cols={cols}: {row:?}");
            }
        }
    }

    #[test]
    fn minimal_and_empty_command_rows_are_safe() {
        for cols in 0..=10u32 {
            let row = format_command_row("", "0", "14ms", cols);
            assert_fits(&row, cols);
            if cols >= 4 {
                assert!(row.ends_with("14ms"), "cols={cols}: {row:?}");
            }
        }
        assert_eq!(format_command_row("", "0", "14ms", 0), "");
        assert_eq!(format_command_row("", "0", "14ms", 1), "…");
        assert_eq!(format_command_row("ls", "0", "14ms", 0), "");
    }

    #[test]
    fn non_ascii_commands_are_bounded_without_corruption() {
        let row = format_command_row("日本語のコマンドを実行する", "0", "14ms", 24);
        assert!(row.ends_with("0 14ms"));
        assert!(row.contains('…'));
        assert_fits(&row, 24);
        assert!(!row.contains('\u{fffd}'));

        let row = format_command_row("café --crème", "0", "14ms", 12);
        assert!(row.ends_with("0 14ms"));
        assert_fits(&row, 12);
    }

    #[test]
    fn footer_keeps_the_meaningful_path_tail() {
        assert_eq!(format_footer("/home/bandit", 24), "cwd /home/bandit");
        assert_eq!(format_footer("—", 24), "cwd —");
        let path = "/home/bandit/Projects/zet/crates/zet/src/window";
        let row = format_footer(path, 24);
        assert!(row.starts_with("cwd …"), "{row:?}");
        assert!(row.ends_with("src/window"), "{row:?}");
        assert_fits(&row, 24);
        for cols in 0..=24u32 {
            let row = format_footer(path, cols);
            assert_fits(&row, cols);
            assert!(!row.contains('\u{0}'));
        }
    }

    #[test]
    fn cache_rebuilds_only_when_dirty_or_changed() {
        let mut cache = ChromeCache::new();
        let mut builds = 0;
        cache.refresh_if_changed(0, false, 0, None, String::new(), 0, true, || {
            builds += 1;
            RailSnapshot {
                mode: "Terminal Focus".to_string(),
                active: 0,
                sessions: Vec::new(),
                commands: Vec::new(),
                footer: "—".to_string(),
                footer_provenance: Provenance::Unknown,
            }
        });
        assert_eq!(builds, 1);
        cache.refresh_if_changed(0, false, 0, None, String::new(), 0, true, || {
            builds += 1;
            RailSnapshot {
                mode: "Terminal Focus".to_string(),
                active: 0,
                sessions: Vec::new(),
                commands: Vec::new(),
                footer: "—".to_string(),
                footer_provenance: Provenance::Unknown,
            }
        });
        assert_eq!(builds, 1);
        cache.mark_dirty();
        cache.refresh_if_changed(0, false, 0, None, String::new(), 0, true, || {
            builds += 1;
            RailSnapshot {
                mode: "Terminal Focus".to_string(),
                active: 0,
                sessions: Vec::new(),
                commands: Vec::new(),
                footer: "—".to_string(),
                footer_provenance: Provenance::Unknown,
            }
        });
        assert_eq!(builds, 2);
    }
}

/// Remove control and escape sequences, collapse line breaks, and bound
/// the result to `max_cells`. This keeps untrusted terminal text from
/// affecting Inspector layout or injecting control sequences.
pub fn sanitize_inspector_text(text: &str, max_cells: usize) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\x07' {
                        break;
                    }
                    if next == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            } else {
                chars.next();
            }
            continue;
        }
        if c == '\n' || c == '\r' || c.is_control() {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    // Collapse runs of whitespace left by removed controls.
    let mut collapsed = String::new();
    let mut last_space = false;
    for c in out.chars() {
        if c.is_whitespace() {
            if !last_space {
                collapsed.push(' ');
            }
            last_space = true;
        } else {
            collapsed.push(c);
            last_space = false;
        }
    }
    truncate_to_cells(collapsed.trim(), max_cells)
}

/// Sanitize recorded command output for bounded Inspector display. Preserves
/// line breaks while removing other control characters and bounding the
/// result.
pub fn sanitize_inspector_output(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c == '\n' {
            out.push('\n');
        } else if c == '\r' || c.is_control() {
            out.push(' ');
        } else {
            out.push(c);
        }
        if out.chars().count() >= max_chars {
            break;
        }
    }
    out
}

/// Read-only snapshot for the first Contextual Inspector slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorSnapshot {
    pub active: usize,
    pub session: String,
    pub cwd: String,
    pub state: String,
    pub exit: String,
    pub commands: Vec<CommandRow>,
}

/// Dirty-flagged cache for Inspector snapshots.
#[derive(Debug, Clone)]
pub struct InspectorCache {
    pub snapshot: Option<Arc<InspectorSnapshot>>,
    pub dirty: bool,
    pub last_session_id: Option<u64>,
    pub last_active: usize,
    pub last_cwd: String,
    pub last_phase: Option<String>,
    pub last_tab_count: usize,
    pub last_active_dead: bool,
    pub last_blocks: usize,
    pub last_refresh: Option<Instant>,
    pub poll_interval: Duration,
}

impl InspectorCache {
    pub fn new() -> Self {
        Self {
            snapshot: None,
            dirty: true,
            last_session_id: None,
            last_active: usize::MAX,
            last_cwd: String::new(),
            last_phase: None,
            last_tab_count: usize::MAX,
            last_active_dead: true,
            last_blocks: usize::MAX,
            last_refresh: None,
            poll_interval: Duration::from_millis(250),
        }
    }

    pub fn snapshot(&self) -> Option<&InspectorSnapshot> {
        self.snapshot.as_deref()
    }

    pub fn snapshot_handle(&self) -> Option<Arc<InspectorSnapshot>> {
        self.snapshot.clone()
    }

    pub fn poll_due(&self, now: Instant) -> bool {
        self.last_refresh
            .is_none_or(|last| now.duration_since(last) >= self.poll_interval)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn cheap_changed(
        &self,
        session_id: u64,
        active: usize,
        phase: Option<zet_pty::shell::Phase>,
        cwd: &str,
        tab_count: usize,
        active_dead: bool,
    ) -> bool {
        let phase = phase.map(|phase| format!("{phase:?}"));
        self.dirty
            || self.last_session_id != Some(session_id)
            || self.last_active != active
            || self.last_tab_count != tab_count
            || self.last_active_dead != active_dead
            || self.last_phase != phase
            || self.last_cwd != cwd
    }

    pub fn mark_refreshed(&mut self, now: Instant) {
        self.last_refresh = Some(now);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_refresh(
        &mut self,
        session_id: u64,
        active: usize,
        phase: Option<zet_pty::shell::Phase>,
        cwd: String,
        tab_count: usize,
        active_dead: bool,
        snapshot: InspectorSnapshot,
        now: Instant,
    ) {
        self.snapshot = Some(Arc::new(snapshot));
        self.last_session_id = Some(session_id);
        self.last_active = active;
        self.last_phase = phase.map(|phase| format!("{phase:?}"));
        self.last_cwd = cwd;
        self.last_tab_count = tab_count;
        self.last_active_dead = active_dead;
        self.last_refresh = Some(now);
        self.dirty = false;
    }
}

impl Default for InspectorCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod inspector_sanitize_tests {
    use super::*;

    #[test]
    fn sanitize_removes_controls_and_bounds_text() {
        assert_eq!(sanitize_inspector_text("a\x1b[31mb\n\rc", 8), "ab c");
        assert_eq!(sanitize_inspector_text("verylongpath", 6), "veryl…");
    }
}

#[cfg(test)]
mod inspector_cache_tests {
    use super::*;

    #[test]
    fn inspector_cache_starts_dirty() {
        let cache = InspectorCache::new();
        assert!(cache.dirty);
        assert!(cache.snapshot().is_none());
    }

    #[test]
    fn inspector_cache_commits_and_clears_dirty() {
        let mut cache = InspectorCache::new();
        let snapshot = InspectorSnapshot {
            active: 0,
            session: "session 1".to_string(),
            cwd: "/tmp".to_string(),
            state: "prompt".to_string(),
            exit: "—".to_string(),
            commands: Vec::new(),
        };
        cache.commit_refresh(
            1,
            0,
            None,
            "/tmp".to_string(),
            1,
            false,
            snapshot.clone(),
            Instant::now(),
        );
        assert!(!cache.dirty);
        assert_eq!(cache.snapshot(), Some(&snapshot));
    }

    #[test]
    fn inspector_cache_rebuilds_on_active_change() {
        let mut cache = InspectorCache::new();
        let snapshot = InspectorSnapshot {
            active: 0,
            session: "session 1".to_string(),
            cwd: "/tmp".to_string(),
            state: "prompt".to_string(),
            exit: "—".to_string(),
            commands: Vec::new(),
        };
        cache.commit_refresh(
            1,
            0,
            None,
            "/tmp".to_string(),
            1,
            false,
            snapshot,
            Instant::now(),
        );
        assert!(cache.cheap_changed(1, 1, None, "/tmp", 1, false));
    }

    #[test]
    fn inspector_cache_detects_same_index_session_replacement() {
        let mut cache = InspectorCache::new();
        let snapshot = InspectorSnapshot {
            active: 0,
            session: "session 1".to_string(),
            cwd: "/tmp".to_string(),
            state: "prompt".to_string(),
            exit: "—".to_string(),
            commands: Vec::new(),
        };
        cache.commit_refresh(
            1,
            0,
            None,
            "/tmp".to_string(),
            1,
            false,
            snapshot,
            Instant::now(),
        );
        assert!(!cache.cheap_changed(1, 0, None, "/tmp", 1, false));
        assert!(cache.cheap_changed(2, 0, None, "/tmp", 1, false));
        assert_eq!(cache.last_session_id, Some(1));
    }
}
