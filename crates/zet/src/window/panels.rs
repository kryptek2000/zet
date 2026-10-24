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
pub fn rail_row_target(row: u32, session_count: usize, viewport_rows: u32) -> Option<usize> {
    if session_count == 0 || viewport_rows < 4 || row < 3 {
        return None;
    }
    let session_row = row as usize - 3;
    if session_row < session_count && (row as usize) < viewport_rows as usize {
        Some(session_row)
    } else {
        None
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
