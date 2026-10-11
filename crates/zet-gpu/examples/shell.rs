// Live shell: real PTY + grid + GPU, keyboard in, pixels out.
// Usage: cargo run -p zet-gpu --example shell
// This is the human smoke test (launcher points here).

use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::Window;
use zet_gpu::input::{is_paste, paste_bytes, translate};
use zet_gpu::Renderer;
use zet_pty::shell::ShellState;
use zet_pty::Session;
use zet_term::Terminal;

const HOOKS: [(&str, &str); 2] = [
    ("bash", "zet-integration.bash"),
    ("zsh", "zet-integration.zsh"),
];

fn shell_program() -> (String, Option<String>) {
    let prog = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let stem = std::path::Path::new(&prog)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let hook = HOOKS
        .iter()
        .find(|(name, _)| *name == stem)
        .map(|(_, file)| format!("{}/../../shell/{file}", env!("CARGO_MANIFEST_DIR")));
    (prog, hook)
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    session: Option<Session>,
    term: Terminal,
    shell_state: ShellState,
    mods: ModifiersState,
    rx: Option<Receiver<Vec<u8>>>,
    cols: usize,
    rows: usize,
    shell_name: String,
    dead: bool,
    erase: u8,
    recorder: Option<zet_blocks::Recorder>,
}

/// Shell's current directory via /proc (unix). Best effort: "" when unknown.
fn shell_cwd(session: &Option<Session>) -> String {
    #[cfg(unix)]
    {
        if let Some(pid) = session.as_ref().and_then(|s| s.child_id()) {
            if let Ok(link) = std::fs::read_link(format!("/proc/{pid}/cwd")) {
                return link.to_string_lossy().into_owned();
            }
        }
    }
    String::new()
}

impl App {
    fn pump(&mut self) {
        let mut died = false;
        if let Some(rx) = &self.rx {
            while let Ok(chunk) = rx.try_recv() {
                self.term.feed(&chunk);
            }
            if let Some(s) = &mut self.session {
                if let Ok(Some(_)) = s.try_exit() {
                    died = true;
                }
            }
        }
        if died {
            self.dead = true;
            if let Some(w) = &self.window {
                w.set_title(&format!("ZET — {} (exited)", self.shell_name));
            }
        }
        for event in self.term.drain_events() {
            self.shell_state.observe(&event);
        }
        if let Some(rec) = &mut self.recorder {
            rec.pump(&mut self.term, &shell_cwd(&self.session));
        }
        if let Some(code) = self.shell_state.last_exit() {
            if let Some(w) = &self.window {
                w.set_title(&format!("ZET — {} (exit {code})", self.shell_name));
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes().with_title(format!("ZET — {}", self.shell_name)),
                )
                .unwrap(),
        );
        let renderer = pollster::block_on(Renderer::new(window.clone())).unwrap();
        let (cw, ch) = renderer.cell_px();
        let size = window.inner_size();
        let cols = ((size.width / cw).max(20) as usize).min(500);
        let rows = ((size.height / ch).max(10) as usize).min(200);
        self.term.resize(cols, rows);
        if let Some(s) = &self.session {
            let _ = s.resize(cols.min(32767) as u16, rows.min(32767) as u16);
        }
        self.cols = cols;
        self.rows = rows;
        self.window = Some(window);
        self.renderer = Some(renderer);
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                // Paste first: Ctrl+Shift+V would otherwise type a SYN byte.
                if is_paste(&event.logical_key, pressed, &self.mods) {
                    if let Ok(mut clip) = arboard::Clipboard::new() {
                        if let Ok(text) = clip.get_text() {
                            if let Some(s) = &mut self.session {
                                let _ = s.write_all(&paste_bytes(&text));
                            }
                        }
                    }
                    return;
                }
                if let Some(mut bytes) = translate(
                    &event.logical_key,
                    event.text.as_deref(),
                    pressed,
                    &self.mods,
                ) {
                    // Send exactly what the line discipline erases with.
                    if bytes == [0x7f] {
                        bytes = vec![self.erase];
                    }
                    if let Some(s) = &mut self.session {
                        let _ = s.write_all(&bytes);
                    }
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    let (cw, ch) = r.cell_px();
                    r.resize(size.width, size.height);
                    let cols = ((size.width / cw).max(20) as usize).min(500);
                    let rows = ((size.height / ch).max(10) as usize).min(200);
                    self.term.resize(cols, rows);
                    if let Some(s) = &self.session {
                        let _ = s.resize(cols.min(32767) as u16, rows.min(32767) as u16);
                    }
                    self.cols = cols;
                    self.rows = rows;
                }
            }
            WindowEvent::RedrawRequested => {
                self.pump();
                if let (Some(r), Some(_)) = (&mut self.renderer, &self.window) {
                    r.draw_terminal(&self.term, self.cols, self.rows);
                    r.render();
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let (prog, hook) = shell_program();
    let shell_name = prog.clone();
    // Guarantee a sane TERM even under GUI launchers.
    let mut session = Session::spawn("env", &["TERM=xterm-256color", &prog, "-i"], 80, 24).unwrap();
    if let Some(hook) = hook {
        let _ = session.write_all(format!("source {hook}\nclear\n").as_bytes());
    }
    let reader = session.clone_reader().unwrap();
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        use std::io::Read;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let event_loop = EventLoop::new().unwrap();
    let session_id = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        std::process::id()
    );
    let recorder = zet_blocks::Store::open_default()
        .map(|store| zet_blocks::Recorder::new(store, &session_id))
        .ok();
    let mut app = App {
        window: None,
        renderer: None,
        session: Some(session),
        term: Terminal::new(80, 24),
        shell_state: ShellState::new(),
        mods: ModifiersState::empty(),
        rx: Some(rx),
        cols: 80,
        rows: 24,
        shell_name,
        dead: false,
        erase: 0x7f,
        recorder,
    };
    if let Some(s) = &app.session {
        app.erase = s.erase_char();
    }
    event_loop.run_app(&mut app).unwrap();
}
