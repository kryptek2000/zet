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
}

/// `zet` as a wake word: `zet <real subcommand>` runs the CLI, but
/// `zet <plain words>` translates the rest (`zet whats the command…`
/// must not usage-error). None = run it (known subcommand, bare `zet`,
/// or not a `zet` line at all).
///
/// Canonical CLI group list — the CLI dispatch (zet/src/main.rs) and
/// this fallback share it. Adding a group? Add the match arm AND this
/// list, or the window mistranslates the new command.
pub const CLI_SUBCOMMANDS: &[&str] = &["auth", "blocks", "journal", "inline", "models"];

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
