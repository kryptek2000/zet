use rusqlite::{params, Connection, OptionalExtension, Result};

pub mod recorder;

pub use recorder::{git_branch, Recorder};

/// The one state root: `$ZET_HOME` when set, else `$HOME/.zet`, else
/// `/tmp/.zet`. Every zet file — blocks, journal, usage log, recipes,
/// schedules, runs — lives under it, so `ZET_HOME` relocates the whole
/// state of a machine in one move.
///
/// This crate owns the resolution. Crates that cannot depend on it
/// (zet-models) repeat the same three steps locally; zet-agent reaches
/// this crate transitively through zet-context and could take a direct
/// edge, but repeats the steps too so the resolution stays in one
/// readable place per crate.
pub fn zet_home() -> std::path::PathBuf {
    if let Some(dir) = std::env::var("ZET_HOME").ok().filter(|s| !s.is_empty()) {
        return std::path::PathBuf::from(dir);
    }
    if let Some(home) = std::env::var("HOME").ok().filter(|s| !s.is_empty()) {
        return std::path::Path::new(&home).join(".zet");
    }
    std::path::PathBuf::from("/tmp/.zet")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: i64,
    pub session: String,
    pub command: String,
    pub output: String,
    pub exit_code: i32,
    pub cwd: String,
    /// Branch the command ran on. Empty when there is none to name: not
    /// a repo, no git binary, detached HEAD, or a repo with no commits.
    pub git_branch: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputPreview {
    pub text: String,
    pub truncated: bool,
}

pub type CommandOutputPreview = OutputPreview;
pub type BoundedOutput = OutputPreview;

impl PartialEq<str> for OutputPreview {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl PartialEq<String> for OutputPreview {
    fn eq(&self, other: &String) -> bool {
        self.text == *other
    }
}

impl std::ops::Deref for OutputPreview {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Store { conn };
        store.init()?;
        Ok(store)
    }

    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let store = Store { conn };
        store.init()?;
        Ok(store)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                session     TEXT NOT NULL,
                command     TEXT NOT NULL,
                output      TEXT NOT NULL DEFAULT '',
                exit_code   INTEGER NOT NULL DEFAULT 0,
                cwd         TEXT NOT NULL DEFAULT '',
                git_branch  TEXT NOT NULL DEFAULT '',
                duration_ms INTEGER NOT NULL DEFAULT 0
            )",
            (),
        )?;
        // Databases in the wild predate git_branch, and CREATE TABLE IF
        // NOT EXISTS leaves an existing table exactly as it is — so add
        // the column when it is missing. Runs on every open, so it must
        // stay idempotent: one PRAGMA, one ALTER only the first time.
        if !self.has_column("git_branch")? {
            self.conn.execute(
                "ALTER TABLE blocks ADD COLUMN git_branch TEXT NOT NULL DEFAULT ''",
                (),
            )?;
        }
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_blocks_session ON blocks (session, id)",
            (),
        )?;
        Ok(())
    }

    fn has_column(&self, name: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare("PRAGMA table_info(blocks)")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let column: String = row.get(1)?;
            if column == name {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn append(&self, block: &Block) -> Result<i64> {
        let duration_ms = i64::try_from(block.duration_ms)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        // Secrets must never reach the store: pasted keys echo through the
        // PTY onto recorded rows (seen live with an OpenRouter key). Scrub
        // command and output at the single write path. The branch goes
        // through the same scrub: it is not a secret, but it is arbitrary
        // user text and this is the one place rows are written.
        let command = scrub_secrets(&block.command);
        let output = scrub_secrets(&block.output);
        let git_branch = scrub_secrets(&block.git_branch);
        // Working-directory metadata is arbitrary user text and may carry
        // token-shaped secrets (e.g. a directory whose name embeds a key).
        // Scrub it at the same single write path as the other fields. This
        // is best-effort: a path containing a secret becomes unusable as a
        // path, which is the intended tradeoff.
        let cwd = scrub_secrets(&block.cwd);
        self.conn.execute(
            "INSERT INTO blocks (session, command, output, exit_code, cwd, git_branch, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                block.session,
                command,
                output,
                block.exit_code,
                cwd,
                git_branch,
                duration_ms,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Bounded, session-scoped preview of a recorded block's output.
    /// Data returned from SQLite is capped before display processing.
    pub fn output_preview(
        &self,
        session: &str,
        id: i64,
        max_chars: usize,
    ) -> Result<Option<OutputPreview>> {
        self.output_preview_impl(session, id, max_chars)
    }

    pub fn preview_output(
        &self,
        session: &str,
        id: i64,
        max_chars: usize,
    ) -> Result<Option<OutputPreview>> {
        self.output_preview_impl(session, id, max_chars)
    }

    pub fn get_output_preview(
        &self,
        session: &str,
        id: i64,
        max_chars: usize,
    ) -> Result<Option<OutputPreview>> {
        self.output_preview_impl(session, id, max_chars)
    }

    pub fn command_output_preview(
        &self,
        session: &str,
        id: i64,
        max_chars: usize,
    ) -> Result<Option<OutputPreview>> {
        self.output_preview_impl(session, id, max_chars)
    }

    pub fn bounded_output(
        &self,
        session: &str,
        id: i64,
        max_chars: usize,
    ) -> Result<Option<OutputPreview>> {
        self.output_preview_impl(session, id, max_chars)
    }

    fn output_preview_impl(
        &self,
        session: &str,
        id: i64,
        max_chars: usize,
    ) -> Result<Option<OutputPreview>> {
        const MAX_PREVIEW_CHARS: usize = 4096;
        const UTF8_BYTES_PER_CHAR: usize = 4;
        const SECRET_MARGIN_BYTES: usize = 1024;
        const SENTINEL: usize = 1;
        let max_chars = max_chars.min(MAX_PREVIEW_CHARS);
        let fetch = max_chars
            .saturating_mul(UTF8_BYTES_PER_CHAR)
            .saturating_add(SECRET_MARGIN_BYTES);
        let fetch_with_sentinel = fetch.saturating_add(SENTINEL);
        // CAST(output AS BLOB) makes substr byte-oriented, so an embedded
        // NUL cannot terminate retrieval. The fetch size is bounded before
        // display processing; SQLite does not need length(output).
        let mut stmt = self.conn.prepare(
            "SELECT CASE
                WHEN output IS NULL THEN NULL
                ELSE COALESCE(substr(CAST(output AS BLOB), 1, ?2), zeroblob(0))
             END
             FROM blocks WHERE id = ?1 AND session = ?3",
        )?;
        // The outer Option is the row-presence contract. The inner Option
        // preserves a genuine SQL NULL (possible in legacy/corrupt stores)
        // as unavailable, while COALESCE above turns SQLite's NULL result for
        // a valid zero-length output into an empty blob.
        let raw: Option<Option<Vec<u8>>> = stmt
            .query_row(
                rusqlite::params![id, fetch_with_sentinel as i64, session],
                |row| row.get(0),
            )
            .optional()?;
        let raw = match raw {
            Some(Some(value)) => value,
            Some(None) | None => return Ok(None),
        };
        let more_data = raw.len() == fetch_with_sentinel;
        let bytes = if raw.len() > fetch {
            &raw[..fetch]
        } else {
            &raw[..]
        };
        // Keep the largest valid UTF-8 prefix so a byte boundary cannot
        // create an invalid string or expose a partial code point.
        let mut valid_len = bytes.len();
        while valid_len > 0 && std::str::from_utf8(&bytes[..valid_len]).is_err() {
            valid_len -= 1;
        }
        let preview = String::from_utf8_lossy(&bytes[..valid_len]).to_string();
        let scrubbed = scrub_secrets(&preview);
        let mut text = String::new();
        for (count, c) in scrubbed.chars().enumerate() {
            if count >= max_chars {
                break;
            }
            text.push(c);
        }
        let truncated = more_data || preview.chars().count() > max_chars;
        Ok(Some(OutputPreview { text, truncated }))
    }

    pub fn get(&self, id: i64) -> Result<Block> {
        self.conn.query_row(
            "SELECT id, session, command, output, exit_code, cwd, git_branch, duration_ms
             FROM blocks WHERE id = ?1",
            params![id],
            row_to_block,
        )
    }

    pub fn list(&self, session: &str) -> Result<Vec<Block>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session, command, output, exit_code, cwd, git_branch, duration_ms
             FROM blocks WHERE session = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session], row_to_block)?;
        rows.collect()
    }

    /// Number of recorded blocks for a session, without loading rows.
    pub fn count(&self, session: &str) -> Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM blocks WHERE session = ?1",
                params![session],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| usize::try_from(count).unwrap_or(0))
    }

    /// The most recent commands for a session, without loading their
    /// output blobs. Returned oldest-first for display.
    pub fn recent_commands(&self, session: &str, limit: usize) -> Result<Vec<Block>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, session, command, exit_code, cwd, git_branch, duration_ms
             FROM blocks WHERE session = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![session, limit as i64], |row| {
            let raw_duration: i64 = row.get(6)?;
            let duration_ms = u64::try_from(raw_duration).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Integer,
                    Box::new(e),
                )
            })?;
            Ok(Block {
                id: row.get(0)?,
                session: row.get(1)?,
                command: row.get(2)?,
                output: String::new(),
                exit_code: row.get(3)?,
                cwd: row.get(4)?,
                git_branch: row.get(5)?,
                duration_ms,
            })
        })?;
        let mut blocks: Vec<Block> = rows.collect::<Result<Vec<_>>>()?;
        blocks.reverse();
        Ok(blocks)
    }

    /// Newest blocks across sessions, most recent last (display order).
    pub fn recent(&self, limit: usize) -> Result<Vec<Block>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session, command, output, exit_code, cwd, git_branch, duration_ms
             FROM blocks ORDER BY id DESC LIMIT ?1",
        )?;
        let rows: Vec<Block> = stmt
            .query_map(params![limit as i64], row_to_block)?
            .collect::<Result<Vec<_>>>()?;
        Ok(rows.into_iter().rev().collect())
    }

    /// Most recent successful command starting with `prefix` (and longer
    /// than it), across all sessions. LIKE wildcards in the prefix are
    /// escaped so they match literally. Empty prefix yields nothing.
    pub fn complete(&self, prefix: &str) -> Result<Option<String>> {
        if prefix.is_empty() {
            return Ok(None);
        }
        let mut escaped = String::with_capacity(prefix.len());
        for c in prefix.chars() {
            if matches!(c, '%' | '_' | '\\') {
                escaped.push('\\');
            }
            escaped.push(c);
        }
        let pattern = format!("{escaped}%");
        let mut stmt = self.conn.prepare(
            "SELECT command FROM blocks
             WHERE command LIKE ?1 ESCAPE '\\' AND command != ?2 AND exit_code = 0
             ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![pattern, prefix], |row| row.get::<_, String>(0))?;
        rows.next().transpose()
    }

    /// `<state root>/blocks.sqlite`, creating the root when missing.
    pub fn open_default() -> std::io::Result<Self> {
        let dir = zet_home();
        std::fs::create_dir_all(&dir)?;
        Self::open(
            dir.join("blocks.sqlite")
                .to_str()
                .ok_or_else(|| std::io::Error::other("non-UTF8 state root"))?,
        )
        .map_err(std::io::Error::other)
    }

    pub fn replay(&self, session: &str) -> Result<String> {
        let mut out = String::new();
        for block in self.list(session)? {
            out.push_str("$ ");
            out.push_str(&block.command);
            out.push('\n');
            out.push_str(&block.output);
            if !block.output.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!(
                "[exit {}, {}ms]\n",
                block.exit_code, block.duration_ms
            ));
        }
        Ok(out)
    }
}

fn row_to_block(row: &rusqlite::Row<'_>) -> Result<Block> {
    let raw_duration: i64 = row.get(7)?;
    let duration_ms = u64::try_from(raw_duration).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Integer, Box::new(e))
    })?;
    Ok(Block {
        id: row.get(0)?,
        session: row.get(1)?,
        command: row.get(2)?,
        output: row.get(3)?,
        exit_code: row.get(4)?,
        cwd: row.get(5)?,
        git_branch: row.get(6)?,
        duration_ms,
    })
}

/// Key prefixes scrubbed from stored text, longest first, with the
/// minimum tail length that counts as a secret (keeps `sk-folders`
/// and short fragments intact).
const SECRET_PREFIXES: &[(&str, usize)] = &[
    ("github_pat_", 16),
    ("sk-or-", 16),
    ("sk-ant-", 16),
    ("sk-proj-", 16),
    ("xoxb-", 16),
    ("xoxp-", 16),
    ("xoxa-", 16),
    ("xoxo-", 16),
    ("Bearer ", 8),
    ("ghp_", 16),
    ("gho_", 16),
    ("AKIA", 16),
    ("sk-", 16),
];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '~' | '+' | '/' | '=' | '.')
}

/// Replace key-like tokens with [REDACTED]. Runs on every stored
/// command and output; history search and ghost text see only the
/// scrubbed form.
pub fn scrub_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let mut hit: Option<usize> = None;
        for (prefix, min_tail) in SECRET_PREFIXES {
            if let Some(tail) = rest.strip_prefix(prefix) {
                let tail_len: usize = tail.chars().take_while(|c| is_token_char(*c)).count();
                if tail_len >= *min_tail {
                    hit =
                        Some(prefix.len() + tail.chars().take_while(|c| is_token_char(*c)).count());
                    break;
                }
            }
        }
        match hit {
            Some(len) => {
                out.push_str("[REDACTED]");
                i += len;
            }
            None => {
                let c = rest.chars().next().unwrap();
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}
