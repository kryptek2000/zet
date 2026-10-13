use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};

pub mod shell;

/// Parse-check a command with `<shell> -n` (stdin script, never
/// executed). Guards the agent/NL line-swap: a syntactically broken
/// model reply must notice, not strand the shell in a continuation
/// prompt. Only Bourne-family shells are checked (their `-n` is
/// parse-only); anything else fails OPEN (old behavior) — this gate is
/// UX, not policy; tiers still decide what runs.
pub fn syntax_ok(shell: &str, cmd: &str) -> bool {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let base = shell.rsplit('/').next().unwrap_or(shell);
    if !matches!(base, "bash" | "zsh" | "sh" | "dash") {
        return true;
    }
    let mut child = match Command::new(shell)
        .arg("-n")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return true,
    };
    let wrote = child
        .stdin
        .as_mut()
        .map(|s| s.write_all(cmd.as_bytes()).is_ok() && s.write_all(b"\n").is_ok())
        .unwrap_or(false);
    // Close stdin so the checker sees EOF (else it waits forever), then
    // always reap: a skipped wait would zombie.
    drop(child.stdin.take());
    let parsed = child.wait().map(|s| s.success()).unwrap_or(false);
    wrote && parsed
}

pub struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl Session {
    pub fn spawn(program: &str, args: &[&str], cols: u16, rows: u16) -> std::io::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(std::io::Error::other)?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(std::io::Error::other)?;
        // take_writer() is single-use: hold the one writer for the session.
        let writer = pair.master.take_writer().map_err(std::io::Error::other)?;
        Ok(Session {
            master: pair.master,
            writer,
            child,
        })
    }

    pub fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    /// A second read handle for a pump thread; closed when the slave exits.
    pub fn clone_reader(&self) -> std::io::Result<Box<dyn Read + Send>> {
        self.master
            .try_clone_reader()
            .map_err(std::io::Error::other)
    }

    pub fn wait_with_output(&mut self, out: &mut Vec<u8>) -> std::io::Result<u32> {
        let mut reader = self
            .master
            .try_clone_reader()
            .map_err(std::io::Error::other)?;
        reader.read_to_end(out)?;
        let status = self.child.wait()?;
        Ok(status.exit_code())
    }

    pub fn try_exit(&mut self) -> std::io::Result<Option<u32>> {
        Ok(self.child.try_wait()?.map(|s| s.exit_code()))
    }

    /// Shell child pid, for /proc/cwd lookup and supervision.
    pub fn child_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// The line discipline's erase character (termios VERASE), for Backspace.
    /// Falls back to DEL when unreadable or disabled.
    #[cfg(unix)]
    pub fn erase_char(&self) -> u8 {
        use nix::sys::termios::SpecialCharacterIndices;
        self.master
            .get_termios()
            .map(|t| t.control_chars[SpecialCharacterIndices::VERASE as usize])
            .filter(|&c| c != 0)
            .unwrap_or(0x7f)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(std::io::Error::other)
    }
}

/// The command position of a line: first token past `VAR=value`
/// assignments, quotes stripped. None for empty / assignment-only
/// lines (those run — they set vars — they just name no command).
pub fn first_word(line: &str) -> Option<String> {
    let mut text = line.trim_start();
    loop {
        // Quoted first token: the command itself has spaces.
        if let Some(rest) = text.strip_prefix('"').or_else(|| text.strip_prefix('\'')) {
            let quote = text.chars().next().unwrap();
            let end = rest.find(quote)?;
            return Some(rest[..end].to_string());
        }
        let tok = text.split_whitespace().next()?;
        // VAR=value prefix (no spaces, valid name before =): skip it.
        if let Some(eq) = tok.find('=') {
            let name = &tok[..eq];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                text = text[tok.len()..].trim_start();
                if text.is_empty() {
                    return None;
                }
                continue;
            }
        }
        return Some(tok.to_string());
    }
}

/// Shell builtins + keywords: always run directly, never translated.
/// Union of bash/zsh everyday words (not exhaustive — PATH covers the
/// rest; an unknown builtin just takes the model path, still safe).
pub fn is_shell_word(name: &str) -> bool {
    matches!(
        name,
        "alias"
            | "bg"
            | "bind"
            | "break"
            | "builtin"
            | "caller"
            | "case"
            | "cd"
            | "command"
            | "compgen"
            | "complete"
            | "continue"
            | "coproc"
            | "declare"
            | "dirs"
            | "disown"
            | "do"
            | "done"
            | "echo"
            | "elif"
            | "else"
            | "enable"
            | "esac"
            | "eval"
            | "exec"
            | "exit"
            | "export"
            | "false"
            | "fc"
            | "fi"
            | "for"
            | "function"
            | "getopts"
            | "hash"
            | "help"
            | "history"
            | "if"
            | "jobs"
            | "kill"
            | "let"
            | "local"
            | "logout"
            | "popd"
            | "printf"
            | "pushd"
            | "pwd"
            | "read"
            | "readonly"
            | "return"
            | "select"
            | "set"
            | "shift"
            | "source"
            | "suspend"
            | "test"
            | "then"
            | "time"
            | "times"
            | "trap"
            | "true"
            | "type"
            | "typeset"
            | "ulimit"
            | "umask"
            | "unalias"
            | "unset"
            | "until"
            | "wait"
            | "while"
            | "whence"
            | "zmodload"
    )
}

/// Commands known runnable without translation: PATH binaries at
/// startup plus shell-reported aliases/functions (via 133;X markers).
pub struct KnownCommands {
    set: std::collections::HashSet<String>,
}

impl KnownCommands {
    pub fn from_env() -> Self {
        let mut set = std::collections::HashSet::new();
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        set.insert(name.to_string());
                    }
                }
            }
        }
        Self { set }
    }

    /// Shell-reported names (aliases/functions from integration hooks).
    pub fn insert_names<I, S>(&mut self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for n in names {
            let n = n.as_ref().trim();
            if !n.is_empty() {
                self.set.insert(n.to_string());
            }
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.set.contains(name)
    }

    /// Closest known name within distance 2, min by (distance, length,
    /// name). None when nothing is close — silence over noise.
    pub fn closest(&self, word: &str) -> Option<String> {
        if word.is_empty() || word.len() > 32 {
            return None;
        }
        let mut best: Option<(usize, String)> = None;
        for name in &self.set {
            if name.len().abs_diff(word.len()) > 2 {
                continue;
            }
            let d = levenshtein(word, name);
            if d == 0 || d > 2 {
                continue;
            }
            let better = match &best {
                None => true,
                Some((bd, bn)) => d < *bd || (d == *bd && (name.len(), name) < (bn.len(), bn)),
            };
            if better {
                best = Some((d, name.clone()));
            }
        }
        best.map(|(_, n)| n)
    }
}

/// Edit distance over chars. Small words only in practice; plain DP.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let old = row[j + 1];
            row[j + 1] = (old + 1).min(row[j] + 1).min(prev + cost);
            prev = old;
        }
    }
    row[b.len()]
}

/// Path the shell complained about: first quoted '…'/"…" on a
/// no-such-file line, else a pathy bare token before the complaint.
pub fn extract_missing_path(output: &str) -> Option<String> {
    for line in output.lines() {
        let low = line.to_lowercase();
        if !(low.contains("no such file or directory") || low.contains("cannot access")) {
            continue;
        }
        for q in ['\'', '"'] {
            if let Some(a) = line.find(q) {
                if let Some(b) = line[a + 1..].find(q) {
                    let s = line[a + 1..a + 1 + b].trim();
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
            }
        }
        // Bare `path: No such…` (unquoted tools): token before the colon
        // that looks like a path.
        if let Some(idx) = low.find("no such file") {
            let before = &line[..idx];
            if let Some(tok) = before
                .split_whitespace()
                .rev()
                .find_map(|w| w.strip_suffix(':'))
            {
                if tok.contains('/') || tok.contains('.') {
                    return Some(tok.to_string());
                }
            }
        }
    }
    None
}

/// Case-correct `candidate` against `cwd` (or absolute): exactly one
/// case-insensitive match renames, else None (missing or ambiguous).
/// Exact hits need no fix (failure was something else) → None.
pub fn fix_path_case(cwd: &str, candidate: &str) -> Option<String> {
    let cand = candidate.trim();
    if cand.is_empty() {
        return None;
    }
    let trailing_slash = cand.ends_with('/');
    let (dir_part, file_part): (&str, &str) = match cand.trim_end_matches('/').rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", cand.trim_end_matches('/')),
    };
    if file_part.is_empty() || file_part == "." || file_part == ".." {
        return None;
    }
    // Home expansion is shell business: skip ~ paths.
    if dir_part == "~" || dir_part.starts_with("~/") {
        return None;
    }
    let base = if dir_part.is_empty() {
        std::path::PathBuf::from(cwd)
    } else if cand.starts_with('/') {
        std::path::PathBuf::from(dir_part)
    } else {
        std::path::Path::new(cwd).join(dir_part)
    };
    if base.join(file_part).exists() {
        return None;
    }
    let entries = std::fs::read_dir(&base).ok()?;
    let want = file_part.to_lowercase();
    let mut hits = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.to_lowercase() == want {
            hits.push(name);
        }
    }
    if hits.len() != 1 {
        return None;
    }
    let fixed = hits.pop().unwrap();
    let mut out = if dir_part.is_empty() {
        fixed
    } else {
        format!("{dir_part}/{fixed}")
    };
    if trailing_slash {
        out.push('/');
    }
    Some(out)
}

/// Corrected command for common failures, else None. 127 + unknown
/// word → closest known command; other nonzero exits + file-not-found
/// output → case fix. Preview-only fuel: the window offers, never runs.
pub fn suggest_fix(
    command: &str,
    cwd: &str,
    output: &str,
    exit_code: i32,
    known: &KnownCommands,
) -> Option<String> {
    if command.trim().is_empty() {
        return None;
    }
    if exit_code == 127 {
        let (word, at) = first_word_at(command)?;
        if known.contains(&word) || is_shell_word(&word) || word == "zet" {
            return None;
        }
        let fix = known.closest(&word)?;
        let mut out = command.to_string();
        out.replace_range(at..at + word.len(), &fix);
        return Some(out);
    }
    if exit_code == 0 {
        return None;
    }
    let cand = extract_missing_path(output)?;
    let fixed = fix_path_case(cwd, &cand)?;
    let idx = command.find(&cand)?;
    let mut out = command.to_string();
    out.replace_range(idx..idx + cand.len(), &fixed);
    Some(out)
}

/// First word plus its byte offset (past assignments/quotes).
fn first_word_at(line: &str) -> Option<(String, usize)> {
    let base = line.len() - line.trim_start().len();
    let mut text = &line[base..];
    let mut at = base;
    loop {
        let t = text.trim_start();
        at += text.len() - t.len();
        text = t;
        if text.is_empty() {
            return None;
        }
        if let Some(rest) = text.strip_prefix('"').or_else(|| text.strip_prefix('\'')) {
            let quote = text.chars().next().unwrap();
            let end = rest.find(quote)?;
            return Some((rest[..end].to_string(), at + 1));
        }
        let tok = text.split_whitespace().next()?;
        if let Some(eq) = tok.find('=') {
            let name = &tok[..eq];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                at += tok.len();
                text = &text[tok.len()..];
                continue;
            }
        }
        return Some((tok.to_string(), at));
    }
}

/// `zet` as a wake word: `zet <real subcommand>` runs the CLI, but
/// `zet <plain words>` translates the rest (`zet whats the command…`
/// must not usage-error). None = run it (known subcommand, bare `zet`,
/// or not a `zet` line at all).
///
/// Canonical CLI group list — the CLI dispatch (zet/src/main.rs) and
/// this fallback share it. Adding a group? Add the match arm AND this
/// list, or the window mistranslates the new command.
pub const CLI_SUBCOMMANDS: &[&str] = &[
    "auth", "blocks", "journal", "inline", "models", "undo", "run", "runs",
];

pub fn zet_fallback(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("zet")?;
    // `zet` glued to more word chars (zeta, zetup) is not us.
    if let Some(c) = rest.chars().next() {
        if !c.is_whitespace() {
            return None;
        }
    }
    let rest = rest.trim_start().to_string();
    if rest.is_empty() {
        return None;
    }
    let second = rest.split_whitespace().next().unwrap_or("");
    if CLI_SUBCOMMANDS.contains(&second) {
        return None;
    }
    Some(rest)
}
