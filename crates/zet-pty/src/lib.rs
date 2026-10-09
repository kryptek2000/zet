use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};

pub struct Session {
    master: Box<dyn MasterPty + Send>,
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
        Ok(Session {
            master: pair.master,
            child,
        })
    }

    pub fn write_all(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut writer = self.master.take_writer().map_err(std::io::Error::other)?;
        writer.write_all(bytes)
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
