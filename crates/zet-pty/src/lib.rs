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
