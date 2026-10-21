use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::JournalEntry;

/// The journal file: `<state root>/journal.jsonl`. The state root is
/// `$ZET_HOME` → `$HOME/.zet` → `/tmp/.zet` — the same resolution
/// `zet_blocks::zet_home()` does, repeated here so this crate's
/// dependency list stays as it is.
pub fn journal_path_default() -> PathBuf {
    let root = match std::env::var("ZET_HOME").ok().filter(|s| !s.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => match std::env::var("HOME").ok().filter(|s| !s.is_empty()) {
            Some(home) => Path::new(&home).join(".zet"),
            None => PathBuf::from("/tmp/.zet"),
        },
    };
    root.join("journal.jsonl")
}

pub fn append_journal(path: &Path, entries: &[JournalEntry]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    for e in entries {
        let line = serde_json::to_string(e).map_err(std::io::Error::other)?;
        writeln!(f, "{line}")?;
    }
    Ok(())
}

pub fn read_journal(path: &Path) -> std::io::Result<Vec<JournalEntry>> {
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<JournalEntry>(&line) {
            out.push(e);
        }
    }
    Ok(out)
}
