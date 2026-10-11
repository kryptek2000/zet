use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::JournalEntry;

pub fn journal_path_default() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    Path::new(&home).join(".zet").join("journal.jsonl")
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
