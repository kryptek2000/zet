//! Mission Rail data model (Slice 4B).
//!
//! The rail is a read-only view over real PTY sessions and observed
//! commands. Every row carries a provenance so the UI never implies more
//! certainty than its source supports. Unknown stays unknown (`—`).

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
    pub shell: String,
    pub state: String,
    pub exit: String,
    pub provenance: Provenance,
}

/// A real recorded command as shown in the rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRow {
    pub command: String,
    pub exit: String,
    pub duration: String,
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

/// Dirty-flagged cache for panel snapshots. The rail is rebuilt at most
/// once per frame and only when something observable changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeCache {
    pub snapshot: Option<RailSnapshot>,
    dirty: bool,
    last_active: usize,
    last_layout_rail: bool,
    last_blocks: usize,
    last_phase: Option<String>,
    last_cwd: String,
    last_tab_count: usize,
    last_active_dead: bool,
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
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn snapshot(&self) -> Option<&RailSnapshot> {
        self.snapshot.as_ref()
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
            self.snapshot = Some(build());
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
    CommandsHeader,
    Command(usize),
    Footer,
    Empty,
}

/// Deterministic rail row layout shared by rendering and hit testing.
pub fn rail_rows(viewport_rows: u32, session_count: usize, command_count: usize) -> Vec<RailRow> {
    let mut rows = vec![RailRow::Empty; viewport_rows as usize];
    if viewport_rows == 0 {
        return rows;
    }
    let footer_row = viewport_rows as usize - 1;
    rows[0] = RailRow::Header;
    if viewport_rows > 1 {
        rows[1] = RailRow::Mode;
    }
    if viewport_rows > 2 && footer_row > 2 {
        rows[2] = RailRow::SessionsHeader;
    }

    let session_start = 3usize;
    let session_capacity = footer_row.saturating_sub(session_start);
    let visible_sessions = session_count.min(session_capacity);
    for i in 0..visible_sessions {
        rows[session_start + i] = RailRow::Session(i);
    }

    let mut cursor = session_start + visible_sessions;
    if command_count > 0 && cursor < footer_row {
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

/// Session index for a clicked rail row, if any.
pub fn rail_session_at_row(row: u32, session_count: usize, viewport_rows: u32) -> Option<usize> {
    let rows = rail_rows(viewport_rows, session_count, 0);
    match rows.get(row as usize) {
        Some(RailRow::Session(index)) => Some(*index),
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
    fn one_session_has_footer_on_last_row() {
        let rows = rail_rows(8, 1, 1);
        assert_eq!(rows[0], RailRow::Header);
        assert_eq!(rows[3], RailRow::Session(0));
        assert_eq!(rows[7], RailRow::Footer);
        assert_eq!(rail_session_at_row(3, 1, 8), Some(0));
        assert_eq!(rail_session_at_row(7, 1, 8), None);
    }

    #[test]
    fn two_sessions_do_not_touch_footer() {
        let rows = rail_rows(8, 2, 1);
        assert_eq!(rows[3], RailRow::Session(0));
        assert_eq!(rows[4], RailRow::Session(1));
        assert_eq!(rows[7], RailRow::Footer);
        assert_eq!(rail_session_at_row(4, 2, 8), Some(1));
    }

    #[test]
    fn five_or_more_sessions_are_bounded() {
        let rows = rail_rows(12, 7, 2);
        let sessions: Vec<_> = rows
            .iter()
            .filter(|r| matches!(r, RailRow::Session(_)))
            .collect();
        assert_eq!(sessions.len(), 7); // rows 3..9, footer row 11
        assert_eq!(rows[11], RailRow::Footer);
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
