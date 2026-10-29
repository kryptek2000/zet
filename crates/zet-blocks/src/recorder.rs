// Block recorder: FinalTerm OSC 133 markers (emitted by shell/zet-integration
// scripts) become Store blocks. Markers carry absolute grid positions
// snapshotted at parse time, so pump timing is free: one pump per frame is
// fine, and positions survive scrolls between feed and pump (up to scrollback
// cap eviction, which shifts absolute rows and is out of scope).
//
// Markers: B = command input starts (cursor = prompt end), C = pre-exec
// (command = prompt row from B column), D;<code> = done (output = absolute
// lines after the command row through D's row, edge-trimmed). D without a
// preceding C (aborted prompt) records nothing.

use std::time::Instant;

use zet_term::{MarkerKind, Terminal};

use crate::{Block, Store};

/// Branch checked out at `cwd`, or empty when there is none to name:
/// not a repo, no git binary, detached HEAD, or a repo with no commits
/// yet. Best effort by construction — a recorded command must never be
/// lost because git misbehaved, so every failure is an empty string.
///
/// Once per finished block (not per keystroke): the branch only moves
/// when the user switches it, and a subprocess per pump would cost more
/// than the whole pump.
pub fn git_branch(cwd: &str) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["branch", "--show-current"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

pub struct Recorder {
    store: Store,
    session: String,
    cmd: Option<(usize, usize)>,
    pending: Option<(String, String)>,
    t0: Option<Instant>,
    /// Shell-reported command names (133;X markers), drained by the
    /// window into its KnownCommands set.
    cmd_names: Vec<String>,
    /// Last nonzero-exit block (command, cwd, output, exit), drained by
    /// the window for did-you-mean offers. Preview fuel only.
    last_failed: Option<(String, String, String, i32)>,
}

impl Recorder {
    pub fn new(store: Store, session: &str) -> Self {
        Recorder {
            store,
            session: session.to_string(),
            cmd: None,
            pending: None,
            t0: None,
            cmd_names: Vec::new(),
            last_failed: None,
        }
    }

    /// Shell-reported names since the last call (aliases/functions).
    pub fn drain_cmd_names(&mut self) -> Vec<String> {
        std::mem::take(&mut self.cmd_names)
    }

    /// Last failed block since the last call, for fix-it offers.
    pub fn drain_failed(&mut self) -> Option<(String, String, String, i32)> {
        self.last_failed.take()
    }

    pub fn blocks(&self) -> rusqlite::Result<Vec<Block>> {
        self.store.list(&self.session)
    }

    /// Number of recorded blocks for this session, without loading rows.
    pub fn block_count(&self) -> usize {
        self.store.count(&self.session).unwrap_or(0)
    }

    /// The most recent recorded commands for this session, without
    /// loading their output blobs.
    pub fn recent_commands(&self, limit: usize) -> rusqlite::Result<Vec<Block>> {
        self.store.recent_commands(&self.session, limit)
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn output_preview(
        &self,
        id: i64,
        max_chars: usize,
    ) -> rusqlite::Result<Option<crate::OutputPreview>> {
        self.store.output_preview(&self.session, id, max_chars)
    }

    pub fn preview_output(
        &self,
        id: i64,
        max_chars: usize,
    ) -> rusqlite::Result<Option<crate::OutputPreview>> {
        self.store.preview_output(&self.session, id, max_chars)
    }

    pub fn get_output_preview(
        &self,
        id: i64,
        max_chars: usize,
    ) -> rusqlite::Result<Option<crate::OutputPreview>> {
        self.store.get_output_preview(&self.session, id, max_chars)
    }

    /// The session key this recorder is bound to.
    pub fn session(&self) -> &str {
        &self.session
    }

    /// Text being typed at the current prompt: (text, prompt column,
    /// absolute prompt row). Some only while typing (after B, before C);
    /// None while executing or idle. Ghost-text source for autocomplete.
    pub fn current_input(&self, term: &Terminal) -> Option<(String, usize, usize)> {
        let (abs, col) = self.cmd?;
        if self.pending.is_some() {
            return None;
        }
        let sb = term.scrollback_len();
        let cur_abs = sb + term.cursor().1;
        let mut text = String::new();
        for r in abs..=cur_abs {
            let line = abs_line_text(term, r);
            let part = if r == abs {
                line.chars().skip(col).collect::<String>()
            } else {
                line
            };
            text.push_str(&part);
        }
        Some((text.trim_end().to_string(), col, abs))
    }

    pub fn pump(&mut self, term: &mut Terminal, cwd: &str) {
        for m in term.drain_markers() {
            match m.kind {
                MarkerKind::Input => {
                    self.cmd = Some((m.abs_row, m.col));
                    self.pending = None;
                    self.t0 = None;
                }
                MarkerKind::Exec => {
                    let Some((b_abs, b_col)) = self.cmd else {
                        continue;
                    };
                    // Real shells put C on the row after the command (the
                    // Enter echo moves down first): the command spans rows
                    // B..C, output never lands there. Join them (wrapped and
                    // PS2-continuation rows alike, newline-joined).
                    // Degenerate C-on-command-row falls back to the snapshot.
                    let command = if m.abs_row == b_abs {
                        m.line
                            .chars()
                            .skip(b_col)
                            .collect::<String>()
                            .trim_end()
                            .to_string()
                    } else {
                        let mut text = String::new();
                        for r in b_abs..m.abs_row {
                            let line = abs_line_text(term, r);
                            let part = if r == b_abs {
                                line.chars().skip(b_col).collect::<String>()
                            } else {
                                line
                            };
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(&part);
                        }
                        text.trim_end().to_string()
                    };
                    if command.is_empty() {
                        self.cmd = None;
                        continue;
                    }
                    self.pending = Some((command, m.line.clone()));
                    self.t0 = Some(Instant::now());
                }
                MarkerKind::Done => self.finish(term, cwd, &m),
                // X: shell-reported names; never a block, just vocab.
                MarkerKind::Cmds => {
                    self.cmd_names
                        .extend(m.line.split_whitespace().map(str::to_string));
                }
            }
        }
    }

    fn finish(&mut self, term: &Terminal, cwd: &str, done: &zet_term::Marker) {
        let (command, c_line) = self.pending.take().unwrap_or_default();
        let (abs, t0) = match (self.cmd.take(), self.t0.take()) {
            (Some((abs, _)), Some(t0)) => (abs, t0),
            _ => return,
        };
        if command.is_empty() {
            return;
        }
        // Rows strictly between command and Done come from the grid (they
        // predate any later prompt). Done's own row uses its parse-time
        // snapshot: the next prompt lands there after D fires, and a lazy
        // read would leak it into output.
        let mut lines: Vec<String> = ((abs + 1)..done.abs_row)
            .map(|r| abs_line_text(term, r))
            .collect();
        let tail = if done.abs_row == abs {
            // Same row: strip the exact prompt+command prefix.
            done.line
                .trim_end()
                .strip_prefix(c_line.trim_end())
                .unwrap_or("")
                .trim_end()
                .to_string()
        } else {
            done.line.trim_end().to_string()
        };
        lines.push(tail);
        while lines.first().is_some_and(|l| l.is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let branch = git_branch(cwd);
        let block = Block {
            id: 0,
            session: self.session.clone(),
            command,
            output: lines.join("\n"),
            exit_code: done.exit_code,
            cwd: cwd.to_string(),
            git_branch: branch,
            duration_ms: t0.elapsed().as_millis() as u64,
        };
        if done.exit_code != 0 {
            self.last_failed = Some((
                block.command.clone(),
                block.cwd.clone(),
                block.output.clone(),
                block.exit_code,
            ));
        }
        let _ = self.store.append(&block);
    }
}

fn abs_line_text(term: &Terminal, abs: usize) -> String {
    let sb = term.scrollback_len();
    let raw = if abs < sb {
        term.scrollback_line(abs).unwrap_or_default()
    } else {
        term.line_text(abs - sb)
    };
    raw.trim_end().to_string()
}
