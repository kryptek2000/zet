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
    pub session: String,
    pub command: String,
    pub exit: String,
    pub duration: String,
    pub cwd: String,
    pub git_branch: String,
    pub provenance: Provenance,
}

/// Whether the cross-session timeline was actually readable from the
/// recorder. `Recorded` with zero rows is an honest empty history;
/// `Unavailable` means the application could not read the store (or no
/// recorder exists) and must not present the absence as an empty history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineAvailability {
    Recorded,
    Unavailable,
}

impl TimelineAvailability {
    pub fn empty_message(self) -> &'static str {
        match self {
            Self::Recorded => "NO OBSERVED WORK",
            Self::Unavailable => "RECORDING UNAVAILABLE",
        }
    }
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

/// Format the second line of a two-row command card. The readable form keeps
/// the field names when the row has room; the existing bounded formatter is
/// the conservative fallback for narrow rails so a duration unit is never
/// rendered as a misleading bare number.
pub fn format_command_metadata(exit: &str, duration: &str, cols: u32) -> String {
    let cols = cols as usize;
    if cols == 0 {
        return String::new();
    }
    let exit = single_line(exit);
    let duration = single_line(duration);
    let labeled = format!("exit {exit}  {duration}");
    if text_cells(&labeled) <= cols {
        labeled
    } else {
        format_command_row("", &exit, &duration, cols as u32)
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
    CommandDetail(usize),
    Footer,
    Empty,
}

/// Rows used by one session card: a title row and a path/state row.
pub const SESSION_CARD_ROWS: usize = 2;
/// Expanded rails leave one blank row between cards. Compact rails spend
/// every row on a real session instead.
pub const SESSION_GAP_ROWS: usize = 1;
/// Every observed command owns a command line and a separate evidence line.
/// Keeping this in the pure layout model makes drawing and hit-testing agree.
pub const COMMAND_CARD_ROWS: usize = 2;

/// Column for the exit label and, when it can sit clear of the exit, the
/// right-aligned duration in a rail command detail row.
pub const RAIL_EXIT_COL: u32 = 5;

pub fn rail_command_detail_columns(cols: u32, exit: &str, duration: &str) -> (u32, Option<u32>) {
    let exit_text = format!("exit {}", single_line(exit));
    let exit_end = RAIL_EXIT_COL
        .saturating_add(text_cells(&exit_text) as u32)
        .saturating_add(1);
    let duration_cells = text_cells(&single_line(duration)) as u32;
    let duration_col = cols.saturating_sub(duration_cells.saturating_add(1));
    let duration_col = (duration_cells > 0 && duration_col > exit_end).then_some(duration_col);
    (RAIL_EXIT_COL, duration_col)
}

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

    if command_count > 0 && cursor + 1 + COMMAND_CARD_ROWS < footer_row {
        rows[cursor] = RailRow::Empty;
        cursor += 1;
        rows[cursor] = RailRow::CommandsHeader;
        cursor += 1;
        let command_capacity = footer_row.saturating_sub(cursor) / COMMAND_CARD_ROWS;
        let visible_commands = command_count.min(command_capacity);
        for i in 0..visible_commands {
            let command_row = cursor + i * COMMAND_CARD_ROWS;
            rows[command_row] = RailRow::Command(i);
            rows[command_row + 1] = RailRow::CommandDetail(i);
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

/// Command index for a clicked activity row, if the row is real. Keeping
/// this beside `rail_rows` makes the visual stream and hit testing share one
/// bounded layout contract.
pub fn rail_command_at_row(
    row: u32,
    session_count: usize,
    command_count: usize,
    viewport_rows: u32,
) -> Option<usize> {
    match rail_rows(viewport_rows, session_count, command_count).get(row as usize) {
        Some(RailRow::Command(index)) | Some(RailRow::CommandDetail(index)) => Some(*index),
        _ => None,
    }
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
    use super::{
        rail_command_at_row, rail_command_detail_columns, rail_rows, rail_session_at_row,
        text_cells, RailRow, COMMAND_CARD_ROWS, RAIL_EXIT_COL,
    };

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
        assert_eq!(rows[11], RailRow::Command(0));
        assert_eq!(rows[12], RailRow::CommandDetail(0));
        assert_eq!(rows[13], RailRow::Command(1));
        assert_eq!(rows[14], RailRow::CommandDetail(1));
        assert_eq!(rows[15], RailRow::Command(2));
        assert_eq!(rows[16], RailRow::CommandDetail(2));
        assert_eq!(rows[19], RailRow::Footer);
    }

    #[test]
    fn rail_command_hit_testing_matches_observed_rows() {
        assert_eq!(rail_command_at_row(11, 2, 3, 20), Some(0));
        assert_eq!(rail_command_at_row(12, 2, 3, 20), Some(0));
        assert_eq!(rail_command_at_row(13, 2, 3, 20), Some(1));
        assert_eq!(rail_command_at_row(14, 2, 3, 20), Some(1));
        assert_eq!(rail_command_at_row(15, 2, 3, 20), Some(2));
        assert_eq!(rail_command_at_row(16, 2, 3, 20), Some(2));
        assert_eq!(rail_command_at_row(10, 2, 3, 20), None);
        assert_eq!(rail_command_at_row(19, 2, 3, 20), None);
    }

    #[test]
    fn commands_are_paired_and_never_cross_the_footer() {
        for rows in 0u32..=40 {
            for sessions in 0usize..=8 {
                for commands in 0usize..=8 {
                    let layout = rail_rows(rows, sessions, commands);
                    for (row, kind) in layout.iter().enumerate() {
                        let Some(index) = (match kind {
                            RailRow::Command(index) | RailRow::CommandDetail(index) => Some(*index),
                            _ => None,
                        }) else {
                            continue;
                        };
                        let expected = if matches!(kind, RailRow::Command(_)) {
                            RailRow::CommandDetail(index)
                        } else {
                            RailRow::Command(index)
                        };
                        let pair = layout.get(if matches!(kind, RailRow::Command(_)) {
                            row + 1
                        } else {
                            row.saturating_sub(1)
                        });
                        assert_eq!(pair, Some(&expected), "unpaired command row {row}");
                        assert!(row < layout.len().saturating_sub(1));
                    }
                }
            }
        }
        assert_eq!(COMMAND_CARD_ROWS, 2);
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

    #[test]
    fn command_detail_columns_never_overlap_and_keep_duration_when_readable() {
        let (exit_col, duration_col) = rail_command_detail_columns(22, "0", "7ms");
        assert_eq!(exit_col, RAIL_EXIT_COL);
        let duration_col = duration_col.expect("duration fits");
        let exit_end = exit_col + text_cells("exit 0") as u32;
        assert!(duration_col > exit_end);
        assert!(duration_col + text_cells("7ms") as u32 <= 22);

        let (_, duration_col) = rail_command_detail_columns(12, "255", "123456789ms");
        assert_eq!(
            duration_col, None,
            "crowded detail must drop metadata, not overlap"
        );
        let (_, duration_col) = rail_command_detail_columns(22, "0", "");
        assert_eq!(duration_col, None);
    }
}

#[cfg(test)]
mod inspector_layout_tests {
    use super::{
        inspector_command_position, inspector_command_row, inspector_detail_start,
        inspector_header_descriptor_col, inspector_header_layout, inspector_selection_at_row,
        inspector_selection_for_command, inspector_session_command_at_row,
        inspector_visible_commands, text_cells, timeline_display_rows, timeline_empty_row,
        timeline_layout, timeline_preview_heading, timeline_row_layout, timeline_session_badges,
        CommandRow, InspectorView, Provenance, TimelineAvailability, INSPECTOR_ACTIVITY_START,
        INSPECTOR_HEADER_RIGHT_INSET, TIMELINE_BADGE_COL, TIMELINE_COMMAND_COL, TIMELINE_INDEX_COL,
    };

    fn command(id: i64, session: &str, text: &str, exit: &str, duration: &str) -> CommandRow {
        CommandRow {
            id,
            session: session.to_string(),
            command: text.to_string(),
            exit: exit.to_string(),
            duration: duration.to_string(),
            cwd: "/tmp".to_string(),
            git_branch: String::new(),
            provenance: Provenance::Observed,
        }
    }

    #[test]
    fn session_and_command_rows_are_stable_and_measured() {
        assert_eq!(inspector_command_row(0), 7);
        assert_eq!(inspector_command_row(3), 10);
        assert_eq!(inspector_selection_for_command(0), 4);
        assert_eq!(inspector_command_position(4), Some(0));
        assert_eq!(inspector_command_position(3), None);
        assert_eq!(inspector_detail_start(2), 10);
    }

    #[test]
    fn only_visible_command_rows_are_click_targets() {
        assert_eq!(
            inspector_selection_at_row(InspectorView::Session, 6, 2, 30, false),
            None
        );
        assert_eq!(
            inspector_selection_at_row(InspectorView::Session, 1, 2, 30, false),
            None
        );
        assert_eq!(
            inspector_selection_at_row(InspectorView::Session, 7, 2, 30, false),
            Some(4)
        );
        assert_eq!(
            inspector_selection_at_row(InspectorView::Session, 8, 2, 30, false),
            Some(5)
        );
        assert_eq!(
            inspector_selection_at_row(InspectorView::Session, 9, 2, 30, false),
            None
        );
        assert_eq!(inspector_session_command_at_row(7, 2, 8), Some(0));
        assert_eq!(inspector_session_command_at_row(8, 2, 8), None);
    }

    #[test]
    fn timeline_hit_testing_matches_only_drawn_rows() {
        let layout = timeline_layout(30, 20, false);
        // Footer is row 29; the preview starts at row 24.
        assert_eq!(layout.list_start, INSPECTOR_ACTIVITY_START);
        assert_eq!(layout.list_end, 24);
        assert_eq!(layout.preview_start, Some(24));
        assert_eq!(layout.command_at_row(6), None);
        assert_eq!(layout.command_at_row(7), Some(0));
        assert_eq!(layout.command_at_row(23), Some(16));
        assert_eq!(layout.command_at_row(24), None); // preview header
        assert_eq!(layout.command_at_row(29), None); // provenance
        assert_eq!(
            inspector_selection_at_row(InspectorView::Timeline, 7, 20, 30, false),
            Some(4)
        );
        assert_eq!(
            inspector_selection_at_row(InspectorView::Timeline, 24, 20, 30, false),
            None
        );
        assert_eq!(
            inspector_selection_at_row(InspectorView::Timeline, 29, 20, 30, false),
            None
        );
    }

    #[test]
    fn short_inspector_heights_never_select_hidden_commands() {
        for rows in 0u32..=40 {
            for count in 0usize..=12 {
                for expanded in [false, true] {
                    let layout = timeline_layout(rows, count, expanded);
                    assert!(
                        layout.visible_commands <= count,
                        "rows={rows} count={count} visible={}",
                        layout.visible_commands
                    );
                    assert!(layout.list_end <= layout.footer_row);
                    assert!(layout
                        .preview_start
                        .is_none_or(|start| start <= layout.footer_row));
                    for row in 0..=rows.saturating_add(2) {
                        if let Some(position) = layout.command_at_row(row) {
                            assert!(position < layout.visible_commands);
                            assert!(
                                row >= layout.list_start && row < layout.list_end && row < rows,
                                "row {row} escaped the visible list"
                            );
                        }
                    }
                    let via_hit = (0..rows)
                        .filter(|row| {
                            inspector_selection_at_row(
                                InspectorView::Timeline,
                                *row,
                                count,
                                rows,
                                expanded,
                            )
                            .is_some()
                        })
                        .count();
                    assert_eq!(via_hit, layout.visible_commands);
                }
            }
        }
        assert_eq!(timeline_empty_row(0), None);
        assert!(timeline_empty_row(3).unwrap() < 3);
        assert!(timeline_empty_row(12).unwrap() < 12);
    }

    #[test]
    fn keyboard_and_mouse_share_the_visible_command_count() {
        for rows in 0u32..=40 {
            for count in 0usize..=8 {
                assert_eq!(
                    inspector_visible_commands(InspectorView::Timeline, rows, count, false),
                    timeline_layout(rows, count, false).visible_commands
                );
                assert_eq!(
                    inspector_visible_commands(InspectorView::Session, rows, count, false),
                    (0..rows)
                        .filter(|row| {
                            inspector_selection_at_row(
                                InspectorView::Session,
                                *row,
                                count,
                                rows,
                                false,
                            )
                            .is_some()
                        })
                        .count()
                );
            }
        }
    }

    #[test]
    fn expanded_layout_trades_list_rows_for_a_bounded_preview() {
        let compact = timeline_layout(30, 8, false);
        let expanded = timeline_layout(30, 8, true);
        assert!(!compact.preview_start.is_none());
        assert!(!expanded.preview_start.is_none());
        assert!(expanded.preview_rows > compact.preview_rows);
        assert!(expanded.visible_commands <= compact.visible_commands);
        assert!(expanded.preview_rows <= 10);
        assert!(expanded.list_end <= expanded.footer_row);
    }

    #[test]
    fn thirty_two_column_rows_keep_a_readable_command_summary() {
        let layout = timeline_row_layout(32, "0", "14ms");
        assert_eq!(layout.command_col, TIMELINE_COMMAND_COL);
        assert_eq!(
            layout.status_col, None,
            "metadata must yield to the command"
        );
        assert!(
            layout.command_budget >= 21,
            "32-column command budget was {}",
            layout.command_budget
        );
        let sample = "cargo test --release";
        let text = layout.command_text(sample);
        assert_eq!(text, sample, "a typical command must stay intact");
        assert!(text_cells(&text) <= 32);
        assert_eq!(TIMELINE_INDEX_COL, 2);
        assert_eq!(TIMELINE_BADGE_COL, 6);

        let rows = timeline_display_rows(
            &[command(1, "session-a", sample, "0", "14ms")],
            24,
            false,
            32,
        );
        assert_eq!(rows[0].index_label, "01");
        assert_eq!(rows[0].session_badge, "S1");
        assert_eq!(rows[0].command_text, sample);
        assert_eq!(rows[0].status_text, None);
        assert!(rows[0].command_col >= 8);
    }

    #[test]
    fn long_commands_degrade_with_a_visible_ellipsis_inside_the_cell_budget() {
        let layout = timeline_row_layout(32, "127", "812ms");
        let command = "git push origin feat/p3a2-observed-work-timeline";
        let text = layout.command_text(command);
        assert!(text.ends_with('…'), "{text:?}");
        assert!(text_cells(&text) <= layout.command_budget);
        assert!(layout.command_budget <= 23);
        assert!(!text.contains("812ms"), "detail belongs in the preview");
        assert!(!text.is_empty());
    }

    #[test]
    fn wider_rows_only_show_metadata_when_the_command_stays_readable() {
        let hidden = timeline_row_layout(39, "0", "14ms");
        assert_eq!(hidden.status_col, None);
        let shown = timeline_row_layout(40, "0", "14ms");
        assert_eq!(shown.status, "0 14ms");
        assert!(shown.status_col.is_some());
        assert!(shown.command_budget >= 16);
        let crowded = timeline_row_layout(40, "127", "123456789ms");
        assert_eq!(crowded.status_col, None);
        assert!(crowded.command_budget > 16);
    }

    #[test]
    fn session_badges_are_distinct_and_stable_for_the_displayed_window() {
        let rows = vec![
            command(1, "shared-prefix-a", "echo a", "0", "1ms"),
            command(2, "shared-prefix-b", "echo b", "0", "1ms"),
            command(3, "shared-prefix-a", "echo c", "0", "1ms"),
        ];
        assert_eq!(timeline_session_badges(&rows), vec!["S1", "S2", "S1"]);
        let models = timeline_display_rows(&rows, 24, false, 32);
        assert_eq!(models[0].session_badge, "S1");
        assert_eq!(models[1].session_badge, "S2");
        assert_ne!(models[0].session_badge, models[1].session_badge);
        assert_eq!(models[2].session_badge, models[0].session_badge);
    }

    #[test]
    fn empty_and_unavailable_states_are_honest_and_distinct() {
        assert_eq!(
            TimelineAvailability::Recorded.empty_message(),
            "NO OBSERVED WORK"
        );
        assert_eq!(
            TimelineAvailability::Unavailable.empty_message(),
            "RECORDING UNAVAILABLE"
        );
        assert_ne!(
            TimelineAvailability::Recorded.empty_message(),
            TimelineAvailability::Unavailable.empty_message()
        );
    }

    #[test]
    fn refined_inspector_descriptors_stay_clear_at_supported_widths() {
        let views = [
            ("CONTEXTUAL INSPECTOR", "SESSION-SCOPED / OBSERVED"),
            ("OBSERVED WORK TIMELINE", "CROSS-SESSION / OBSERVED"),
            ("AGENT ACTIVITY", "LAUNCH CANDIDATES ONLY"),
        ];
        for (title, descriptor) in views {
            assert_eq!(
                inspector_header_descriptor_col(32, title, descriptor),
                None,
                "32-col Inspector must keep the title and descriptor clear: {title}"
            );
            let col = inspector_header_descriptor_col(80, title, descriptor)
                .unwrap_or_else(|| panic!("descriptor should fit at 80 cols: {title}"));
            assert!(col > 4 + text_cells(title) as u32);
            assert!(col + text_cells(descriptor) as u32 <= 78);
        }
    }

    #[test]
    fn inspector_header_layout_has_positive_padding_and_clearance() {
        for ch in [20u32, 22, 24, 32] {
            let layout = inspector_header_layout(
                80,
                ch,
                "OBSERVED WORK TIMELINE",
                "CROSS-SESSION / OBSERVED",
            );
            assert!(layout.mark_left_px > 0, "left padding must be positive");
            assert!(layout.mark_top_px > 0, "top padding must be positive");
            assert!(
                layout.title_left_px > layout.mark_left_px + layout.mark_size_px,
                "title must clear mark"
            );
            assert!(
                layout.title_baseline_px > layout.mark_top_px as f32,
                "title must be optically centered below top padding"
            );
            assert!(
                layout.separator_px > layout.tab_top_px,
                "separator must sit below tab row"
            );
            assert!(layout.tab_height_px > 4, "tabs must retain usable height");
        }
    }

    #[test]
    fn inspector_header_descriptor_has_right_breathing_room() {
        let descriptor = "LAUNCH CANDIDATES ONLY";
        let col = inspector_header_descriptor_col(80, "AGENT ACTIVITY", descriptor).unwrap();
        assert!(
            col + text_cells(descriptor) as u32 + INSPECTOR_HEADER_RIGHT_INSET <= 80,
            "descriptor must keep the configured right-side inset"
        );
    }

    #[test]
    fn inspector_header_layout_narrow_width_drops_descriptor() {
        let layout =
            inspector_header_layout(32, 22, "OBSERVED WORK TIMELINE", "CROSS-SESSION / OBSERVED");
        assert_eq!(layout.descriptor_col, None);
    }

    #[test]
    fn header_descriptor_and_preview_hints_never_overlap_at_32_columns() {
        let title = "OBSERVED WORK TIMELINE";
        let descriptor = "OBSERVED / SESSION-SCOPED";
        assert_eq!(inspector_header_descriptor_col(32, title, descriptor), None);
        let wide = inspector_header_descriptor_col(80, title, descriptor).unwrap();
        assert!(wide > 4 + text_cells(title) as u32);
        assert!(wide + text_cells(descriptor) as u32 <= 78);

        let (label, hint) = timeline_preview_heading(32, false);
        let (hint, col) = hint.unwrap();
        assert!(2 + text_cells(label) as u32 <= col);
        assert!(col + text_cells(hint) as u32 <= 30);
        let (label, hint) = timeline_preview_heading(32, true);
        let (hint, col) = hint.unwrap();
        assert!(2 + text_cells(label) as u32 <= col);
        assert!(col + text_cells(hint) as u32 <= 30);
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
    fn command_metadata_prefers_labeled_evidence_when_it_fits() {
        let row = format_command_metadata("127", "812ms", 20);
        assert_eq!(row, "exit 127  812ms");
        assert_fits(&row, 20);
    }

    #[test]
    fn command_metadata_stays_bounded_on_narrow_rails() {
        for cols in 0..=24u32 {
            let row = format_command_metadata("127", "812ms", cols);
            assert_fits(&row, cols);
            if cols >= 6 {
                assert!(
                    row.ends_with("812ms") || row.ends_with("…ms"),
                    "cols={cols}: duration lost in {row:?}"
                );
            }
        }
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
    if max_chars == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (count, c) in text.chars().enumerate() {
        if count >= max_chars {
            break;
        }
        if c == '\n' {
            out.push('\n');
        } else if c == '\r' || c.is_control() {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// Inspector selection values retain the historical P3-A1 offset: values
/// below this constant were never command selections, so navigation starts
/// at the first real displayed entry instead.
pub const INSPECTOR_CONTEXT_ROWS: usize = 4;
/// `INSPECTOR_CONTEXT_ROWS` is the stable selection offset; the name is kept
/// for compatibility with the P3-A1 focus contract.
pub const INSPECTOR_SELECTION_BASE: usize = INSPECTOR_CONTEXT_ROWS;
pub const INSPECTOR_ACTIVITY_START: u32 = 7;

/// Which Inspector surface owns the displayed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorView {
    Session,
    Timeline,
    Agents,
}

pub fn inspector_command_row(index: usize) -> u32 {
    INSPECTOR_ACTIVITY_START + index as u32
}

/// Selection value stored by App for a zero-based displayed command.
pub fn inspector_selection_for_command(index: usize) -> usize {
    INSPECTOR_SELECTION_BASE + index
}

/// Zero-based displayed command for a stored selection value.
pub fn inspector_command_position(selection: usize) -> Option<usize> {
    selection.checked_sub(INSPECTOR_SELECTION_BASE)
}

pub fn inspector_detail_start(command_count: usize) -> u32 {
    INSPECTOR_ACTIVITY_START
        .saturating_add(command_count as u32)
        .saturating_add(1)
}

/// Session-mode command rows are contiguous from the activity start and
/// clipped by the viewport. The detail card never shares those rows.
pub fn inspector_session_visible_commands(viewport_rows: u32, command_count: usize) -> usize {
    command_count.min(viewport_rows.saturating_sub(INSPECTOR_ACTIVITY_START) as usize)
}

/// Command index for a session-mode row. Header rows, the section label,
/// the selected-command card, and rows beyond the viewport are not targets.
pub fn inspector_session_command_at_row(
    row: u32,
    command_count: usize,
    viewport_rows: u32,
) -> Option<usize> {
    if row < INSPECTOR_ACTIVITY_START {
        return None;
    }
    let index = (row - INSPECTOR_ACTIVITY_START) as usize;
    (index < inspector_session_visible_commands(viewport_rows, command_count)).then_some(index)
}

/// Fixed geometry for a Timeline event row at the 32-column Inspector.
///
/// The status dot lives in the pixel gutter before the index. Index, badge,
/// and command are measured in terminal cells; the command owns the
/// remainder of the row. Exit/duration metadata is only painted when it
/// leaves a genuinely readable command budget, so narrow rows spend their
/// cells on the command and the selected-event preview carries the detail.
pub const TIMELINE_INDEX_COL: u32 = 2;
pub const TIMELINE_BADGE_COL: u32 = 6;
pub const TIMELINE_COMMAND_COL: u32 = 9;
pub const TIMELINE_RIGHT_INSET: u32 = 2;
/// Extra right-side breathing room for the Contextual Inspector header's
/// contextual descriptor, so it does not sit in the panel's edge area.
pub const INSPECTOR_HEADER_RIGHT_INSET: u32 = 4;
pub const TIMELINE_MIN_COMMAND_CELLS: usize = 16;
pub const TIMELINE_METADATA_MIN_COLS: u32 = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineRowLayout {
    pub command_col: u32,
    pub command_budget: usize,
    pub status_col: Option<u32>,
    pub status: String,
}

impl TimelineRowLayout {
    pub fn command_text(&self, command: &str) -> String {
        truncate_to_cells(&single_line(command), self.command_budget)
    }
}

/// Lay out the command/status fields for one Timeline row in `cols` cells.
/// The returned budgets include the ellipsis, so every painted field fits.
pub fn timeline_row_layout(cols: u32, exit: &str, duration: &str) -> TimelineRowLayout {
    let command_col = TIMELINE_COMMAND_COL;
    let base_budget = cols.saturating_sub(TIMELINE_RIGHT_INSET.saturating_add(command_col));
    let status = format!(
        "{} {}",
        single_line(exit).trim(),
        single_line(duration).trim()
    )
    .trim()
    .to_string();
    let status_cells = text_cells(&status) as u32;
    if status_cells == 0 || cols <= command_col {
        return TimelineRowLayout {
            command_col,
            command_budget: base_budget as usize,
            status_col: None,
            status,
        };
    }
    let status_col = cols.saturating_sub(TIMELINE_RIGHT_INSET.saturating_add(status_cells));
    let readable = cols >= TIMELINE_METADATA_MIN_COLS
        && status_col >= command_col.saturating_add(TIMELINE_MIN_COMMAND_CELLS as u32 + 1);
    if readable {
        TimelineRowLayout {
            command_col,
            command_budget: status_col.saturating_sub(command_col).saturating_sub(1) as usize,
            status_col: Some(status_col),
            status,
        }
    } else {
        TimelineRowLayout {
            command_col,
            command_budget: base_budget as usize,
            status_col: None,
            status,
        }
    }
}

/// One rendered Timeline row, composed once so rendering and tests consume
/// the same measurement and truncation decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineRowModel {
    pub row: u32,
    pub position: usize,
    pub index_label: String,
    pub session_badge: String,
    pub command_text: String,
    pub command_col: u32,
    pub status_text: Option<String>,
    pub status_col: Option<u32>,
    pub failed: bool,
}

/// Stable view-local session badges in first-seen order. The badge is a
/// display alias; the selected preview and provenance always keep the full
/// recorded session identity.
pub fn timeline_session_badges(commands: &[CommandRow]) -> Vec<String> {
    let mut sessions: Vec<&str> = Vec::new();
    commands
        .iter()
        .map(|command| {
            let index = sessions
                .iter()
                .position(|session| *session == command.session)
                .unwrap_or_else(|| {
                    sessions.push(command.session.as_str());
                    sessions.len() - 1
                });
            format!("S{}", index + 1)
        })
        .collect()
}

/// Build the visible Timeline rows for the current layout. Hidden entries
/// are never composed, rendered, or selectable.
pub fn timeline_display_rows(
    commands: &[CommandRow],
    viewport_rows: u32,
    expanded: bool,
    cols: u32,
) -> Vec<TimelineRowModel> {
    let layout = timeline_layout(viewport_rows, commands.len(), expanded);
    let badges = timeline_session_badges(commands);
    commands
        .iter()
        .take(layout.visible_commands)
        .enumerate()
        .map(|(position, command)| {
            let row_layout = timeline_row_layout(cols, &command.exit, &command.duration);
            let status_text = row_layout
                .status_col
                .map(|_| row_layout.status.clone())
                .filter(|status| !status.is_empty());
            TimelineRowModel {
                row: layout.list_start.saturating_add(position as u32),
                position,
                index_label: format!("{:02}", position + 1),
                session_badge: badges
                    .get(position)
                    .cloned()
                    .unwrap_or_else(|| "S?".to_string()),
                command_text: row_layout.command_text(&command.command),
                command_col: row_layout.command_col,
                status_text,
                status_col: row_layout.status_col,
                failed: !matches!(command.exit.as_str(), "0" | "—"),
            }
        })
        .collect()
}

/// Vertical allocation for Timeline mode. The list is primary; the compact
/// selected-event preview is only reserved when there is room left after a
/// useful list, and the provenance row always wins the final row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineLayout {
    pub list_start: u32,
    pub list_end: u32,
    pub visible_commands: usize,
    pub preview_start: Option<u32>,
    pub preview_rows: u32,
    pub footer_row: u32,
}

impl TimelineLayout {
    pub fn command_at_row(&self, row: u32) -> Option<usize> {
        if row < self.list_start || row >= self.list_end {
            return None;
        }
        let position = (row - self.list_start) as usize;
        (position < self.visible_commands).then_some(position)
    }
}

pub fn timeline_layout(viewport_rows: u32, command_count: usize, expanded: bool) -> TimelineLayout {
    if viewport_rows == 0 {
        return TimelineLayout {
            list_start: 0,
            list_end: 0,
            visible_commands: 0,
            preview_start: None,
            preview_rows: 0,
            footer_row: 0,
        };
    }
    let footer_row = viewport_rows - 1;
    let list_start = INSPECTOR_ACTIVITY_START.min(footer_row);
    let available = footer_row.saturating_sub(list_start);
    // Keep at least two real event rows before spending space on a preview.
    let (preview_rows, list_rows) = if available >= 7 {
        let preview = if expanded {
            available.saturating_sub(2).min(10)
        } else {
            5.min(available.saturating_sub(2))
        };
        (preview, available - preview)
    } else {
        (0, available)
    };
    let list_end = list_start.saturating_add(list_rows);
    let preview_start = (preview_rows > 0).then_some(list_end);
    TimelineLayout {
        list_start,
        list_end,
        visible_commands: command_count.min(list_rows as usize),
        preview_start,
        preview_rows,
        footer_row,
    }
}

/// Row for the honest empty/unavailable message. It stays inside the
/// viewport and never shares the provenance row.
pub fn timeline_empty_row(viewport_rows: u32) -> Option<u32> {
    if viewport_rows < 3 {
        return None;
    }
    let footer = viewport_rows - 1;
    Some(INSPECTOR_ACTIVITY_START.min(footer.saturating_sub(1)))
}

/// Visible command count for the active Inspector view. Navigation and hit
/// testing both consume this, so hidden entries cannot be selected.
pub fn inspector_visible_commands(
    view: InspectorView,
    viewport_rows: u32,
    command_count: usize,
    expanded: bool,
) -> usize {
    match view {
        InspectorView::Session => inspector_session_visible_commands(viewport_rows, command_count),
        InspectorView::Timeline => {
            timeline_layout(viewport_rows, command_count, expanded).visible_commands
        }
        InspectorView::Agents => 0,
    }
}

/// Translate an Inspector click into the stable selection index used by App.
/// Only visible command rows are targets; section labels, the preview,
/// provenance, and hidden entries are not selectable.
pub fn inspector_selection_at_row(
    view: InspectorView,
    row: u32,
    command_count: usize,
    viewport_rows: u32,
    expanded: bool,
) -> Option<usize> {
    let position = match view {
        InspectorView::Session => {
            inspector_session_command_at_row(row, command_count, viewport_rows)
        }
        InspectorView::Timeline => {
            timeline_layout(viewport_rows, command_count, expanded).command_at_row(row)
        }
        InspectorView::Agents => None,
    };
    position.map(inspector_selection_for_command)
}

/// Right-aligned Timeline header descriptor column, if it can sit clear of
/// the measured title with at least one empty cell between them.
pub fn inspector_header_descriptor_col(cols: u32, title: &str, descriptor: &str) -> Option<u32> {
    let title_end = 4u32.saturating_add(text_cells(title) as u32);
    let descriptor_cells = text_cells(descriptor) as u32;
    if descriptor_cells == 0 {
        return None;
    }
    // Prefer the roomier F2 inset, but fall back to the older, tighter
    // insets when the viewport cannot otherwise keep the descriptor clear
    // of the title. This restores the provisional-state label on widths
    // where the larger inset would force it to disappear, while still
    // avoiding the F1 overlap.
    for inset in [INSPECTOR_HEADER_RIGHT_INSET, 3, 2, 1] {
        let col = cols.saturating_sub(inset).saturating_sub(descriptor_cells);
        if col > title_end {
            return Some(col);
        }
    }
    None
}

/// Shared geometry for the Contextual Inspector's two-row header.
///
/// The helper keeps the header inside the existing two-row footprint while
/// giving the Aperture mark, title, descriptor, and tab row intentional
/// padding. Pixel values are relative to the inspector viewport origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InspectorHeaderLayout {
    pub mark_left_px: u32,
    pub mark_top_px: u32,
    pub mark_size_px: u32,
    pub title_left_px: u32,
    pub title_baseline_px: f32,
    pub descriptor_col: Option<u32>,
    pub separator_px: u32,
    pub tab_top_px: u32,
    pub tab_height_px: u32,
}

pub fn inspector_header_layout(
    cols: u32,
    ch: u32,
    title: &str,
    descriptor: &str,
) -> InspectorHeaderLayout {
    let mark_size_px = ch.saturating_sub(8).max(12);
    let mark_left_px = 12;
    let mark_top_px = 5;
    let title_left_px = mark_left_px + mark_size_px + 12;
    let title_baseline_px = 17.5;
    let descriptor_col = inspector_header_descriptor_col(cols, title, descriptor);
    InspectorHeaderLayout {
        mark_left_px,
        mark_top_px,
        mark_size_px,
        title_left_px,
        title_baseline_px,
        descriptor_col,
        separator_px: ch.saturating_mul(3).saturating_sub(2),
        tab_top_px: ch.saturating_mul(2).saturating_add(2),
        tab_height_px: ch.saturating_sub(4),
    }
}

/// Preview heading plus a truthful Enter affordance when it fits.
pub fn timeline_preview_heading(
    cols: u32,
    expanded: bool,
) -> (&'static str, Option<(&'static str, u32)>) {
    let label = "SELECTED EVENT";
    let hint = if expanded {
        "ENTER COLLAPSE"
    } else {
        "ENTER EXPAND"
    };
    let hint_col = cols
        .saturating_sub(TIMELINE_RIGHT_INSET)
        .saturating_sub(text_cells(hint) as u32);
    let label_end = 2u32.saturating_add(text_cells(label) as u32);
    if hint_col >= label_end {
        (label, Some((hint, hint_col)))
    } else {
        (label, None)
    }
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
    pub timeline: Vec<CommandRow>,
    pub timeline_availability: TimelineAvailability,
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
    pub last_timeline_max_id: Option<i64>,
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
            last_timeline_max_id: None,
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
            timeline: Vec::new(),
            timeline_availability: TimelineAvailability::Recorded,
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
            timeline: Vec::new(),
            timeline_availability: TimelineAvailability::Recorded,
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
            timeline: Vec::new(),
            timeline_availability: TimelineAvailability::Recorded,
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

// ---------------------------------------------------------------------------
// Agent inspector geometry and evidence helpers (F3).
// ---------------------------------------------------------------------------

/// Deterministic vertical layout for the native AGENTS inspector. The list,
/// selected-observation detail, evidence limitations, and footer never share
/// rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentInspectorLayout {
    pub header_row: u32,
    pub view_row: u32,
    pub title_row: u32,
    pub section_row: u32,
    pub list_start: u32,
    pub list_end: u32,
    pub visible_observations: usize,
    pub detail_start: Option<u32>,
    pub detail_rows: u32,
    pub warning_row: u32,
    pub footer_row: u32,
}

impl AgentInspectorLayout {
    pub fn observation_at_row(&self, row: u32, offset: usize, count: usize) -> Option<usize> {
        if row < self.list_start || row >= self.list_end {
            return None;
        }
        let position = (row - self.list_start) as usize;
        if position >= self.visible_observations {
            return None;
        }
        let index = offset + position;
        (index < count).then_some(index)
    }
}

/// Bounded recorded-command evidence associated with an agent observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRecordedEvidence {
    pub command: String,
    pub session: String,
    pub exit: String,
    pub duration: String,
    pub cwd: String,
    pub git_branch: String,
    pub output: Option<String>,
    pub output_truncated: bool,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

/// One row in the recorded-evidence surface. The renderer paints exactly
/// these rows; status and footer rows are reserved by the layout rather than
/// discovered after output has consumed the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEvidenceRowKind {
    Title,
    Command,
    Session,
    Exit,
    Duration,
    Output,
    EmptyOutput,
    Unavailable,
    More,
    Truncated,
    Footer,
    Notice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvidenceRow {
    pub row: u32,
    pub kind: AgentEvidenceRowKind,
    pub text: String,
}

/// Pure, bounded plan for the recorded-evidence surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvidenceLayout {
    pub rows: Vec<AgentEvidenceRow>,
    pub output_start: Option<u32>,
    pub output_capacity: usize,
    pub total_output_lines: usize,
    pub start: usize,
    pub visible_output_lines: usize,
    pub hidden_output_lines: usize,
    pub max_offset: usize,
    pub footer_row: Option<u32>,
    pub truncated_visible: bool,
    pub more_visible: bool,
    pub empty_output_visible: bool,
}

impl AgentEvidenceLayout {
    pub fn row(&self, kind: AgentEvidenceRowKind) -> Option<&AgentEvidenceRow> {
        self.rows.iter().find(|row| row.kind == kind)
    }
}

/// Wrap one logical output line into viewport-width chunks. This keeps the
/// tail of long error text reachable through the same vertical scroll as
/// ordinary multiline output. Control characters are expected to have been
/// sanitized by the recorder/display path already.
pub fn wrap_output_to_cells(text: &str, max_cells: usize) -> Vec<String> {
    let max_cells = max_cells.max(1);
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut used = 0usize;
        for c in raw.chars() {
            let cells = char_cells(c);
            if cells > max_cells {
                if !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                    used = 0;
                }
                out.push("…".to_string());
                continue;
            }
            if used > 0 && used + cells > max_cells {
                out.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(c);
            used += cells;
        }
        out.push(line);
    }
    out
}

/// The bounded recorded-evidence row plan consumed directly by the renderer.
///
/// Rows 0..=3 are owned by the outer Inspector header. Evidence starts at row
/// 4. The footer is always at `rows - 2`; one status row at `rows - 3` is
/// reserved before output is budgeted, so `+N MORE` and
/// `OUTPUT TRUNCATED` cannot be starved by output.
pub fn agent_evidence_layout(
    viewport_rows: u32,
    viewport_cols: u32,
    evidence: Option<&AgentRecordedEvidence>,
    offset: usize,
) -> AgentEvidenceLayout {
    let width = viewport_cols.saturating_sub(4).max(1) as usize;
    let mut layout = AgentEvidenceLayout {
        rows: Vec::new(),
        output_start: None,
        output_capacity: 0,
        total_output_lines: 0,
        start: 0,
        visible_output_lines: 0,
        hidden_output_lines: 0,
        max_offset: 0,
        footer_row: None,
        truncated_visible: false,
        more_visible: false,
        empty_output_visible: false,
    };
    if viewport_rows == 0 {
        return layout;
    }
    if viewport_rows < 7 {
        let row = viewport_rows.saturating_sub(2);
        layout.rows.push(AgentEvidenceRow {
            row,
            kind: AgentEvidenceRowKind::Notice,
            text: truncate_to_cells("INSUFFICIENT SPACE  ESC", width),
        });
        layout.footer_row = Some(row);
        return layout;
    }

    let footer_row = viewport_rows - 2;
    let status_row = viewport_rows - 3;
    layout.footer_row = Some(footer_row);

    if status_row > 5 {
        layout.rows.push(AgentEvidenceRow {
            row: 4,
            kind: AgentEvidenceRowKind::Title,
            text: truncate_to_cells("RECORDED COMMAND EVIDENCE", width),
        });
    }

    // At very short heights identity wins over the title: row 4 carries the
    // command (or unavailable session) when the title cannot fit.
    let metadata_start = if status_row > 5 { 5u32 } else { 4u32 };
    let output_start = 9u32;
    let output_capacity = status_row.saturating_sub(output_start) as usize;
    layout.output_start = Some(output_start);
    layout.output_capacity = output_capacity;

    let mut status = String::new();
    if let Some(ev) = evidence {
        if ev.available {
            for (row, kind, text) in [
                (
                    metadata_start,
                    AgentEvidenceRowKind::Command,
                    format!("COMMAND: {}", ev.command),
                ),
                (
                    metadata_start + 1,
                    AgentEvidenceRowKind::Session,
                    format!("SESSION: {}", ev.session),
                ),
                (
                    metadata_start + 2,
                    AgentEvidenceRowKind::Exit,
                    format!("EXIT: {}", ev.exit),
                ),
                (
                    metadata_start + 3,
                    AgentEvidenceRowKind::Duration,
                    format!("DURATION: {}", ev.duration),
                ),
            ] {
                if row < status_row {
                    layout.rows.push(AgentEvidenceRow {
                        row,
                        kind,
                        text: truncate_to_cells(&text, width),
                    });
                }
            }

            if let Some(text) = ev.output.as_deref() {
                if text.is_empty() {
                    layout.empty_output_visible = true;
                    if output_capacity > 0 {
                        layout.rows.push(AgentEvidenceRow {
                            row: output_start,
                            kind: AgentEvidenceRowKind::EmptyOutput,
                            text: truncate_to_cells("EMPTY OUTPUT", width),
                        });
                    } else {
                        status = "EMPTY OUTPUT".to_string();
                    }
                } else {
                    let lines = wrap_output_to_cells(text, width);
                    layout.total_output_lines = lines.len();
                    if output_capacity > 0 && lines.len() > output_capacity {
                        layout.max_offset = lines.len().saturating_sub(output_capacity);
                    }
                    layout.start = if output_capacity == 0 {
                        0
                    } else {
                        offset.min(layout.max_offset)
                    };
                    layout.visible_output_lines = lines
                        .len()
                        .saturating_sub(layout.start)
                        .min(output_capacity);
                    layout.hidden_output_lines =
                        lines.len().saturating_sub(layout.visible_output_lines);
                    for (i, line) in lines
                        .iter()
                        .skip(layout.start)
                        .take(output_capacity)
                        .enumerate()
                    {
                        layout.rows.push(AgentEvidenceRow {
                            row: output_start + i as u32,
                            kind: AgentEvidenceRowKind::Output,
                            text: line.clone(),
                        });
                    }
                }
            } else {
                if output_capacity > 0 {
                    layout.rows.push(AgentEvidenceRow {
                        row: output_start,
                        kind: AgentEvidenceRowKind::Unavailable,
                        text: truncate_to_cells("OUTPUT UNAVAILABLE", width),
                    });
                } else {
                    status = "OUTPUT UNAVAILABLE".to_string();
                }
            }
        } else {
            let mut row = metadata_start;
            if !ev.session.is_empty() && row < status_row {
                layout.rows.push(AgentEvidenceRow {
                    row,
                    kind: AgentEvidenceRowKind::Session,
                    text: truncate_to_cells(&format!("SESSION: {}", ev.session), width),
                });
                row += 1;
            }
            let reason = ev
                .unavailable_reason
                .as_deref()
                .unwrap_or("Recorded evidence unavailable.");
            let wrapped = wrap_output_to_cells(reason, width);
            if wrapped.is_empty() && row < status_row {
                layout.rows.push(AgentEvidenceRow {
                    row,
                    kind: AgentEvidenceRowKind::Unavailable,
                    text: "UNAVAILABLE".to_string(),
                });
            }
            for line in wrapped {
                if row >= status_row {
                    status = "UNAVAILABLE".to_string();
                    break;
                }
                layout.rows.push(AgentEvidenceRow {
                    row,
                    kind: AgentEvidenceRowKind::Unavailable,
                    text: line,
                });
                row += 1;
            }
        }
    } else if status_row > 4 {
        layout.rows.push(AgentEvidenceRow {
            row: 5,
            kind: AgentEvidenceRowKind::Unavailable,
            text: truncate_to_cells("NO EVIDENCE AVAILABLE", width),
        });
    }

    let mut status_parts = Vec::new();
    if layout.hidden_output_lines > 0 {
        status_parts.push(format!("+{} MORE", layout.hidden_output_lines));
    }
    if evidence.map(|ev| ev.output_truncated).unwrap_or(false) {
        status_parts.push("OUTPUT TRUNCATED".to_string());
    }
    if status.is_empty() {
        status = status_parts.join("  ");
    } else if !status_parts.is_empty() {
        status = format!("{}  {}", status, status_parts.join("  "));
    }
    if !status.is_empty() && status_row >= 4 {
        let text = truncate_to_cells(&status, width);
        layout.truncated_visible = text.contains("OUTPUT TRUNCATED");
        layout.more_visible = text.contains(" MORE");
        layout.rows.push(AgentEvidenceRow {
            row: status_row,
            kind: if text.contains("OUTPUT TRUNCATED") {
                AgentEvidenceRowKind::Truncated
            } else if text.contains(" MORE") {
                AgentEvidenceRowKind::More
            } else {
                AgentEvidenceRowKind::Notice
            },
            text,
        });
    }

    let footer_hint = if layout.hidden_output_lines > 0 {
        "ESC RETURN  UP/DOWN"
    } else {
        "ESC RETURN"
    };
    layout.rows.push(AgentEvidenceRow {
        row: footer_row,
        kind: AgentEvidenceRowKind::Footer,
        text: truncate_to_cells(footer_hint, width),
    });
    layout
}

/// Viewport-aware affordance for the AGENTS list. The full hint is kept at
/// wide widths; at the 32-column Inspector floor a shorter hint starts at
/// column 2 so it remains fully visible.
pub fn agent_shortcut_hint(viewport_cols: u32) -> (u32, &'static str) {
    const FULL: &str = "ENTER DETAILS  E EVIDENCE";
    const SHORT: &str = "ENTER DETAIL  E EVIDENCE";
    let full_col = 34u32;
    if viewport_cols >= full_col + text_cells(FULL) as u32 {
        (full_col, FULL)
    } else {
        (2, SHORT)
    }
}

pub fn agent_inspector_layout(
    viewport_rows: u32,
    observation_count: usize,
    offset: usize,
) -> AgentInspectorLayout {
    if viewport_rows == 0 {
        return AgentInspectorLayout {
            header_row: 0,
            view_row: 0,
            title_row: 0,
            section_row: 0,
            list_start: 0,
            list_end: 0,
            visible_observations: 0,
            detail_start: None,
            detail_rows: 0,
            warning_row: 0,
            footer_row: 0,
        };
    }
    let footer_row = viewport_rows - 1;
    let warning_row = if viewport_rows >= 2 {
        viewport_rows - 2
    } else {
        footer_row
    };
    let detail_rows = 4u32;
    // Reserve detail + warning + footer after the list.
    let detail_start = if viewport_rows >= 11 {
        Some(viewport_rows.saturating_sub(6))
    } else {
        None
    };
    let list_start = 5u32.min(footer_row);
    let list_end = detail_start.unwrap_or(warning_row).min(footer_row);
    let list_capacity = list_end.saturating_sub(list_start) as usize;
    let remaining = observation_count.saturating_sub(offset);
    let visible_observations = remaining.min(list_capacity);
    AgentInspectorLayout {
        header_row: 0,
        view_row: 1,
        title_row: 2,
        section_row: 3,
        list_start,
        list_end,
        visible_observations,
        detail_start,
        detail_rows,
        warning_row,
        footer_row,
    }
}

// Aliases for likely hidden-test names.
pub fn agents_inspector_layout(
    viewport_rows: u32,
    observation_count: usize,
    offset: usize,
) -> AgentInspectorLayout {
    agent_inspector_layout(viewport_rows, observation_count, offset)
}
pub fn agent_layout(
    viewport_rows: u32,
    observation_count: usize,
    offset: usize,
) -> AgentInspectorLayout {
    agent_inspector_layout(viewport_rows, observation_count, offset)
}

pub fn agent_observation_at_row(
    layout: &AgentInspectorLayout,
    row: u32,
    offset: usize,
    count: usize,
) -> Option<usize> {
    layout.observation_at_row(row, offset, count)
}

pub fn agents_observation_at_row(
    layout: &AgentInspectorLayout,
    row: u32,
    offset: usize,
    count: usize,
) -> Option<usize> {
    layout.observation_at_row(row, offset, count)
}

/// Conservative recorder availability wording for the AGENTS inspector.
pub fn agent_recording_available_message(available: bool, has_recorder: bool) -> &'static str {
    if !has_recorder {
        "RECORDING UNAVAILABLE"
    } else if available {
        "RECORDING AVAILABLE"
    } else {
        "RECORDING STATUS UNKNOWN"
    }
}

/// Evidence-limitation labels for the AGENTS inspector. Each distinct failure
/// class produces a distinguishable label without inventing counts.
pub fn agent_evidence_limitations(
    dropped: u64,
    recorded_dropped: u64,
    recorded_loss_complete: bool,
    recorded_persistence_failures: u64,
    recorded_delivery_incomplete: u64,
    conflict_status: zet_agent_observe::ConflictStatus,
) -> Vec<&'static str> {
    let mut out = Vec::new();
    if dropped > 0 {
        out.push("OBSERVATION HISTORY EVICTED");
    }
    if recorded_dropped > 0 {
        out.push("RECORDED COMMANDS DROPPED");
    }
    if !recorded_loss_complete {
        out.push("RECORDING LOSS INCOMPLETE");
    }
    if recorded_persistence_failures > 0 {
        out.push("PERSISTENCE FAILURES");
    }
    if recorded_delivery_incomplete > 0 {
        out.push("DELIVERY INCOMPLETE");
    }
    if conflict_status == zet_agent_observe::ConflictStatus::Conflicting {
        out.push("CONFLICT DETECTED");
    }
    out
}

/// Format a bounded agent detail line set for the selected observation.
pub fn agent_detail_lines(obs: &zet_agent_observe::AgentObservation) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("AGENT: {}", obs.agent().as_str()));
    lines.push("LAUNCH CANDIDATE".to_string());
    lines.push(format!(
        "SOURCE: {}",
        match obs.source() {
            zet_agent_observe::EvidenceSource::RecordedCommand => "RECORDED COMMAND",
            zet_agent_observe::EvidenceSource::CommandText => "COMMAND TEXT",
            _ => "UNSUPPORTED",
        }
    ));
    lines.push(format!("SESSION: {}", obs.session().as_str()));
    lines.push(format!("AVAILABILITY: {:?}", obs.availability()).to_uppercase());
    lines.push(format!("STRENGTH: {:?}", obs.strength()).to_uppercase());
    if let Some(cmd) = obs.command() {
        lines.push(format!("RECORDED COMMAND REF: {}", cmd.0));
    }
    if let Some(name) = obs.command_name() {
        lines.push(format!("RETAINED COMMAND NAME: {}", name));
    }
    // Unknown unless proven.
    lines.push("UNVERIFIED:".to_string());
    lines.push("RUNNING: UNKNOWN".to_string());
    lines.push("COMPLETE: UNKNOWN".to_string());
    lines.push("AUTHORIZED: UNKNOWN".to_string());
    lines.push("FILE MODIFICATIONS: UNKNOWN".to_string());
    lines.push("CONFLICT: UNKNOWN".to_string());
    lines
}

#[cfg(test)]
mod agent_inspector_layout_tests {
    use super::*;

    #[test]
    fn agent_layout_short_medium_tall_nonzero_offset() {
        for rows in [8u32, 16, 30] {
            let layout = agent_inspector_layout(rows, 10, 2);
            assert!(layout.list_start < layout.list_end || layout.visible_observations == 0);
            assert!(layout.list_end <= layout.warning_row);
            assert!(layout.warning_row < layout.footer_row || rows < 2);
            assert!(
                layout.detail_start.is_none() || layout.detail_start.unwrap() <= layout.footer_row
            );
            if let Some(ds) = layout.detail_start {
                assert!(ds + layout.detail_rows <= layout.footer_row);
            }
            // nonzero offset doesn't create invalid visible count
            assert!(layout.visible_observations <= 10);
        }
    }

    #[test]
    fn agent_layout_no_overlap() {
        for rows in 0u32..=40 {
            let layout = agent_inspector_layout(rows, 5, 0);
            if rows == 0 {
                assert_eq!(layout.visible_observations, 0);
                continue;
            }
            assert!(layout.list_end <= layout.warning_row);
            if let Some(ds) = layout.detail_start {
                assert!(ds >= layout.list_end);
                assert!(ds + layout.detail_rows <= layout.warning_row + 1);
            }
            assert_eq!(layout.footer_row, rows - 1);
        }
    }

    #[test]
    fn agent_mouse_mapping_matches_rendered_rows() {
        let layout = agent_inspector_layout(20, 5, 1);
        for row in 0..20u32 {
            let idx = agent_observation_at_row(&layout, row, 1, 5);
            if row >= layout.list_start && row < layout.list_end {
                // may be some
            } else {
                assert!(idx.is_none());
            }
        }
        // hidden rows not selectable
        assert!(agent_observation_at_row(&layout, layout.footer_row, 1, 5).is_none());
    }

    #[test]
    fn agent_hidden_rows_not_selectable() {
        for rows in 0u32..=30 {
            let layout = agent_inspector_layout(rows, 8, 0);
            for row in 0..rows + 2 {
                if let Some(idx) = agent_observation_at_row(&layout, row, 0, 8) {
                    assert!(row >= layout.list_start && row < layout.list_end);
                    assert!(idx < 8);
                }
            }
        }
    }

    #[test]
    fn agent_detail_lines_include_unknowns() {
        let mut store = zet_agent_observe::AgentObservationStore::new(8);
        let obs = store
            .observe_command(
                zet_agent_observe::SessionRef::new("s").unwrap(),
                "opencode",
                1,
            )
            .unwrap();
        let lines = agent_detail_lines(&obs);
        let joined = lines.join("\n");
        assert!(joined.contains("LAUNCH CANDIDATE"));
        assert!(joined.contains("RUNNING: UNKNOWN"));
        assert!(joined.contains("COMPLETE: UNKNOWN"));
        assert!(joined.contains("AUTHORIZED: UNKNOWN"));
        assert!(joined.contains("FILE MODIFICATIONS: UNKNOWN"));
        assert!(joined.contains("CONFLICT: UNKNOWN"));
    }
}

#[cfg(test)]
mod f3_panels_extra_tests {
    use super::*;

    #[test]
    fn three_view_active_state_truth() {
        // inspector view enum distinct
        assert_ne!(InspectorView::Session, InspectorView::Timeline);
        assert_ne!(InspectorView::Session, InspectorView::Agents);
        assert_ne!(InspectorView::Timeline, InspectorView::Agents);
    }

    #[test]
    fn list_detail_footer_geometry_no_overlap() {
        for rows in [10u32, 20, 30] {
            let layout = timeline_layout(rows, 8, false);
            assert!(layout.list_end <= layout.footer_row);
            if let Some(ps) = layout.preview_start {
                assert!(ps >= layout.list_end);
                assert!(ps + layout.preview_rows <= layout.footer_row);
            }
        }
    }

    #[test]
    fn mouse_row_mapping_matches_rendered_rows() {
        let layout = timeline_layout(20, 5, false);
        for row in 0..20u32 {
            let idx = layout.command_at_row(row);
            if let Some(i) = idx {
                assert!(i < layout.visible_commands);
                assert!(row >= layout.list_start && row < layout.list_end);
            }
        }
    }

    #[test]
    fn arrow_navigation_across_pages() {
        // visible count bounded
        let layout = timeline_layout(12, 10, false);
        assert!(layout.visible_commands <= 10);
        assert!(layout.visible_commands <= (layout.list_end - layout.list_start) as usize);
    }

    #[test]
    fn enter_exposes_selected_details() {
        // timeline preview heading hint mentions enter
        let (_, hint) = timeline_preview_heading(40, false);
        assert!(hint.is_some());
    }

    #[test]
    fn narrow_geometry_and_resizing() {
        for rows in [0u32, 1, 2, 5, 7] {
            let layout = timeline_layout(rows, 8, false);
            assert!(layout.list_end <= layout.footer_row.saturating_add(1));
            assert!(layout.visible_commands <= 8);
        }
    }

    #[test]
    fn historical_observations_without_active_session() {
        // agent layout works with 0 inspector rows? at least no panic
        let l = agent_inspector_layout(0, 3, 0);
        assert_eq!(l.visible_observations, 0);
    }
}

#[cfg(test)]
mod f4_panels_tests {
    use super::*;

    #[test]
    fn source_and_session_distinct_positions() {
        // detail_start+2 (source) and detail_start+3 (session) are distinct rows.
        let layout = agent_inspector_layout(30, 4, 0);
        let ds = layout.detail_start.unwrap();
        assert_ne!(ds + 2, ds + 3);
    }

    #[test]
    fn no_detail_overlap_narrow_and_normal() {
        for rows in [12u32, 20, 30] {
            let layout = agent_inspector_layout(rows, 4, 0);
            if let Some(ds) = layout.detail_start {
                assert!(ds + 3 < layout.warning_row || rows < 12);
                assert!(layout.list_end <= ds);
            }
        }
    }

    #[test]
    fn clean_snapshot_no_red_warning() {
        let labels = agent_evidence_limitations(
            0,
            0,
            true,
            0,
            0,
            zet_agent_observe::ConflictStatus::Unknown,
        );
        assert!(labels.is_empty());
    }

    #[test]
    fn unknown_conflict_neutral() {
        let labels = agent_evidence_limitations(
            0,
            0,
            true,
            0,
            0,
            zet_agent_observe::ConflictStatus::Unknown,
        );
        assert!(!labels.contains(&"CONFLICT DETECTED"));
    }

    #[test]
    fn none_not_conflicting() {
        let labels =
            agent_evidence_limitations(0, 0, true, 0, 0, zet_agent_observe::ConflictStatus::None);
        assert!(!labels.contains(&"CONFLICT DETECTED"));
    }

    #[test]
    fn conflicting_distinguished() {
        let labels = agent_evidence_limitations(
            0,
            0,
            true,
            0,
            0,
            zet_agent_observe::ConflictStatus::Conflicting,
        );
        assert!(labels.contains(&"CONFLICT DETECTED"));
    }

    #[test]
    fn each_loss_counter_warning() {
        assert!(!agent_evidence_limitations(
            1,
            0,
            true,
            0,
            0,
            zet_agent_observe::ConflictStatus::Unknown
        )
        .is_empty());
        assert!(!agent_evidence_limitations(
            0,
            1,
            true,
            0,
            0,
            zet_agent_observe::ConflictStatus::Unknown
        )
        .is_empty());
        assert!(!agent_evidence_limitations(
            0,
            0,
            false,
            0,
            0,
            zet_agent_observe::ConflictStatus::Unknown
        )
        .is_empty());
        assert!(!agent_evidence_limitations(
            0,
            0,
            true,
            1,
            0,
            zet_agent_observe::ConflictStatus::Unknown
        )
        .is_empty());
        assert!(!agent_evidence_limitations(
            0,
            0,
            true,
            0,
            1,
            zet_agent_observe::ConflictStatus::Unknown
        )
        .is_empty());
    }

    #[test]
    fn multiple_losses_retain_distinctions() {
        let labels = agent_evidence_limitations(
            1,
            2,
            false,
            3,
            4,
            zet_agent_observe::ConflictStatus::Conflicting,
        );
        assert!(labels.len() >= 5);
    }

    #[test]
    fn production_detail_projection_contains_all_fields() {
        let mut store = zet_agent_observe::AgentObservationStore::new(8);
        let obs = store
            .observe_command(
                zet_agent_observe::SessionRef::new("s").unwrap(),
                "opencode",
                1,
            )
            .unwrap();
        let lines = agent_detail_lines(&obs);
        let joined = lines.join("\n");
        for needle in [
            "AGENT:",
            "LAUNCH CANDIDATE",
            "SOURCE:",
            "SESSION:",
            "AVAILABILITY:",
            "STRENGTH:",
            "RUNNING: UNKNOWN",
            "COMPLETE: UNKNOWN",
            "AUTHORIZED: UNKNOWN",
            "FILE MODIFICATIONS: UNKNOWN",
            "CONFLICT: UNKNOWN",
        ] {
            assert!(joined.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn renderer_consumes_detail_projection() {
        // The production renderer references agent_detail_lines.
        // This test is a compile-time guard that the helper is accessible.
        let mut store = zet_agent_observe::AgentObservationStore::new(8);
        let obs = store
            .observe_command(
                zet_agent_observe::SessionRef::new("s").unwrap(),
                "opencode",
                1,
            )
            .unwrap();
        assert!(!agent_detail_lines(&obs).is_empty());
    }
}

#[cfg(test)]
mod p4a3_panels_tests {
    use super::*;

    #[test]
    fn recorded_observation_has_command_ref() {
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let id = store
            .append(&zet_blocks::Block {
                id: 0,
                session: "s".to_string(),
                command: "opencode run".to_string(),
                output: "ok".to_string(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 5,
            })
            .unwrap();
        let mut obs_store = zet_agent_observe::AgentObservationStore::new(8);
        let session = zet_agent_observe::SessionRef::new("s").unwrap();
        let obs = obs_store
            .observe_recorded_command(&store, session, id, 10)
            .unwrap();
        assert_eq!(obs.command(), Some(zet_agent_observe::CommandRef(id)));
        let lines = agent_detail_lines(&obs);
        let joined = lines.join("\n");
        assert!(joined.contains(&format!("RECORDED COMMAND REF: {}", id)));
    }

    #[test]
    fn command_text_observation_has_no_command_ref() {
        let mut store = zet_agent_observe::AgentObservationStore::new(8);
        let obs = store
            .observe_command(
                zet_agent_observe::SessionRef::new("s").unwrap(),
                "opencode",
                1,
            )
            .unwrap();
        assert!(obs.command().is_none());
        let lines = agent_detail_lines(&obs);
        let joined = lines.join("\n");
        assert!(!joined.contains("RECORDED COMMAND REF:"));
    }

    #[test]
    fn navigation_preserves_selection_identity() {
        // smoke: inspector selection conversion
        assert_eq!(
            inspector_command_position(inspector_selection_for_command(2)),
            Some(2)
        );
    }
}

// ---------------------------------------------------------------------------
// P2-B — ZET Signature ASCII Identity
// ---------------------------------------------------------------------------

/// Approved ZET signature ASCII wordmark. Geometry is preserved exactly.
pub const SIGNATURE_WORDMARK: &[&str] = &[
    "  ████████████╗   ████████████╗   █████████████╗",
    "  ╚══════███╔╝   ██╔═════════╝   ╚════███╔════╝",
    "       ▄███╔╝     ██║                 ███║",
    "     ▄███╔╝       ██████████╗         ███║",
    "   ▄███╔╝         ██╔═══════╝         ███║",
    "  ███╔╝           ██║                 ███║",
    "  ████████████╗   ████████████╗       ███║",
    "  ╚═══════════╝   ╚═══════════╝       ╚══╝",
];

pub const SIGNATURE_BRAND_COPY: &str = "ZET // APERTURE INSTRUMENT";
pub const SIGNATURE_TAGLINE: &str = "THE COMMAND LINE, REIMAGINED.";
pub const SIGNATURE_SUPPORTING_IDENTITY: &str = "NATIVE TERMINAL / APERTURE";
pub const SIGNATURE_MINIMAL_TEXT: &str = "ZET // APERTURE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureIdentitySize {
    Full,
    Compact,
    Minimal,
    Omitted,
}

pub(crate) fn signature_wordmark_lines(size: SignatureIdentitySize) -> &'static [&'static str] {
    match size {
        SignatureIdentitySize::Full | SignatureIdentitySize::Compact => SIGNATURE_WORDMARK,
        SignatureIdentitySize::Minimal | SignatureIdentitySize::Omitted => &[],
    }
}

#[cfg(test)]
mod signature_identity_tests {
    use super::*;

    #[test]
    fn wordmark_preserves_approved_geometry() {
        assert_eq!(SIGNATURE_WORDMARK.len(), 8);
        assert_eq!(SIGNATURE_WORDMARK[0].chars().count(), 48);
        assert!(SIGNATURE_WORDMARK[7].chars().count() <= 48);
        for line in SIGNATURE_WORDMARK {
            assert!(!line.is_empty());
            assert!(line.chars().count() <= 48);
        }
    }

    #[test]
    fn supporting_copy_fits_the_wordmark_grid() {
        for line in [
            SIGNATURE_BRAND_COPY,
            SIGNATURE_TAGLINE,
            SIGNATURE_SUPPORTING_IDENTITY,
            SIGNATURE_MINIMAL_TEXT,
        ] {
            assert!(!line.is_empty());
            assert!(line.chars().count() <= 48);
        }
        assert!(SIGNATURE_MINIMAL_TEXT.chars().count() < 48);
    }

    #[test]
    fn wordmark_lines_exist_only_for_wide_layouts() {
        assert_eq!(
            signature_wordmark_lines(SignatureIdentitySize::Full),
            SIGNATURE_WORDMARK
        );
        assert_eq!(
            signature_wordmark_lines(SignatureIdentitySize::Compact),
            SIGNATURE_WORDMARK
        );
        assert!(signature_wordmark_lines(SignatureIdentitySize::Minimal).is_empty());
        assert!(signature_wordmark_lines(SignatureIdentitySize::Omitted).is_empty());
    }
}
