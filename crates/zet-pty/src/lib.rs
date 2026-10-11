use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};

pub mod shell;

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
