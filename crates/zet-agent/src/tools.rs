// Native read-only tools for agent turns: read_file, glob, grep.
// They ride the same function-calling interface as run_shell but execute
// in-process — no shell quoting, no alias traps (`cat` -> `bat` can never
// hang these), structured results. Confined to the turn workdir; the
// shell remains the path for anything outside it (with approval).

use std::path::{Path, PathBuf};
use zet_models::ToolDef;

pub const READ_CAP_BYTES: usize = 32_768;
pub const GLOB_CAP: usize = 200;
pub const GREP_CAP: usize = 50;
pub const GREP_LINE_CAP: usize = 300;
const WALK_FILE_CAP: usize = 5000;
const SKIP_DIRS: [&str; 3] = [".git", "target", "node_modules"];

pub fn read_tool_def() -> ToolDef {
    ToolDef {
        name: "read_file".to_string(),
        description: "Read a text file inside the working directory. Capped; binary refused."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "max_bytes": {"type": "number"},
            },
            "required": ["path"],
        }),
    }
}

pub fn glob_tool_def() -> ToolDef {
    ToolDef {
        name: "glob".to_string(),
        description:
            "List files matching a glob (*, ?) inside the working directory, relative paths."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"pattern": {"type": "string"}},
            "required": ["pattern"],
        }),
    }
}

pub fn grep_tool_def() -> ToolDef {
    ToolDef {
        name: "grep".to_string(),
        description: "Search file contents for a literal substring (not regex) inside the working directory. path:line: text."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "include": {"type": "string"},
            },
            "required": ["pattern"],
        }),
    }
}

/// Resolve a tool path against the workdir. Err when it escapes
/// (absolute elsewhere, or ../ out).
fn resolve(workdir: &Path, path: &str) -> Result<PathBuf, String> {
    let joined = workdir.join(path);
    let base = workdir
        .canonicalize()
        .map_err(|e| format!("cannot resolve working directory: {e}"))?;
    let target = if joined.exists() {
        joined
            .canonicalize()
            .map_err(|e| format!("cannot resolve {path}: {e}"))?
    } else {
        // Nonexistent: normalize lexically (no file to canonicalize).
        let mut clean = PathBuf::new();
        for comp in joined.components() {
            use std::path::Component::*;
            match comp {
                CurDir => {}
                ParentDir => {
                    if !clean.pop() {
                        return Err(format!("path escapes the working directory: {path}"));
                    }
                }
                _ => clean.push(comp.as_os_str()),
            }
        }
        clean
    };
    if !target.starts_with(&base) {
        return Err(format!("path escapes the working directory: {path}"));
    }
    Ok(target)
}

pub fn read_file(workdir: &Path, path: &str, max_bytes: Option<u64>) -> Result<String, String> {
    let target = resolve(workdir, path)?;
    let bytes = std::fs::read(&target).map_err(|e| format!("cannot read {path}: {e}"))?;
    if bytes.contains(&0) {
        return Err(format!("{path} looks binary, refusing"));
    }
    let cap = max_bytes.unwrap_or(READ_CAP_BYTES as u64).min(1_048_576) as usize;
    let text = String::from_utf8_lossy(&bytes);
    if bytes.len() > cap {
        let cut: String = text.chars().take(cap).collect();
        Ok(format!("{cut}\n…(truncated {} bytes total)", bytes.len()))
    } else {
        Ok(text.into_owned())
    }
}

/// `*` spans anything (including `/`), `?` one char. Enough for
/// `*.rs`, `src/*.rs`, `**/*.rs`.
fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = path.chars().collect();
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < s.len() {
        if pi < pat.len() && (pat[pi] == '?' || pat[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

fn walk(workdir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![workdir.to_path_buf()];
    let mut visited = 0;
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > WALK_FILE_CAP || out.len() >= WALK_FILE_CAP {
                return out;
            }
            let path = entry.path();
            // Never follow symlinks (loops); skip vendored bulk.
            if entry.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            if path.is_dir() {
                let skip = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| SKIP_DIRS.contains(&n) || n.starts_with('.'));
                if !skip {
                    stack.push(path);
                }
            } else {
                out.push(path);
            }
        }
    }
    out
}

fn relative(workdir: &Path, path: &Path) -> String {
    path.strip_prefix(workdir)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

pub fn glob(workdir: &Path, pattern: &str) -> Result<String, String> {
    let base = workdir
        .canonicalize()
        .map_err(|e| format!("cannot resolve working directory: {e}"))?;
    let mut hits: Vec<String> = walk(&base)
        .iter()
        .map(|p| relative(&base, p))
        .filter(|rel| glob_match(pattern, rel))
        .collect();
    hits.sort();
    hits.truncate(GLOB_CAP);
    if hits.is_empty() {
        Ok("(no matches)".to_string())
    } else {
        Ok(hits.join("\n"))
    }
}

pub fn grep(workdir: &Path, pattern: &str, include: Option<&str>) -> Result<String, String> {
    let base = workdir
        .canonicalize()
        .map_err(|e| format!("cannot resolve working directory: {e}"))?;
    let mut hits = Vec::new();
    for path in walk(&base) {
        if hits.len() >= GREP_CAP {
            hits.push("(capped at 50 matches)".to_string());
            break;
        }
        let rel = relative(&base, &path);
        if let Some(glob_pat) = include {
            if !glob_match(glob_pat, &rel) {
                continue;
            }
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bytes.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            if line.contains(pattern) {
                let short: String = line.chars().take(GREP_LINE_CAP).collect();
                hits.push(format!("{}:{}: {}", rel, i + 1, short));
                if hits.len() >= GREP_CAP {
                    break;
                }
            }
        }
    }
    if hits.is_empty() {
        Ok("(no matches)".to_string())
    } else {
        Ok(hits.join("\n"))
    }
}
