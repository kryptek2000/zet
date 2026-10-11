// Live shell: real PTY + grid + GPU, keyboard in, pixels out.
// Usage: cargo run -p zet-gpu --example shell
// This is the human smoke test (launcher points here).

use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::Window;
use zet_gpu::input::{is_nl_toggle, is_paste, paste_bytes, translate};
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
    inline_spec: Option<String>,
    ghost: zet_suggest::Ghost,
    nl_mode: bool,
    nl_rx: Option<Receiver<Option<String>>>,
    last_frame: std::time::Instant,
    needs_draw: bool,
}

/// Heartbeat for the NL-answer poll and missed-wake safety net. PTY
/// output and input wake the loop immediately (proxy + needs_draw);
/// idleness costs one cheap render per beat, not sixty per second.
const FRAME: std::time::Duration = std::time::Duration::from_millis(250);

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
                let nl = if self.nl_mode { " (nl)" } else { "" };
                w.set_title(&format!("ZET — {} (exit {code}){nl}", self.shell_name));
            }
        }
        self.update_ghost();
    }

    /// Refresh the visible ghost suffix from current input. History only
    /// (sync, sub-ms); the model runs solely on NL Enter, in a thread.
    fn update_ghost(&mut self) {
        // NL answer landed: swap the NL line for the command (the preview).
        if let Some(rx) = &self.nl_rx {
            if let Ok(answer) = rx.try_recv() {
                self.nl_rx = None;
                if let Some(cmd) = answer {
                    self.accept_line(&cmd);
                } else if let Some(input) = self.current_text() {
                    self.ghost.notify(input, "(no command)".to_string());
                }
                self.nl_mode = false;
                self.retitle();
            } else {
                self.ghost.waiting();
                return;
            }
        }
        let input = self.prompt_input();
        self.ghost.update(input);
    }

    fn current_text(&self) -> Option<String> {
        self.recorder
            .as_ref()
            .and_then(|rec| rec.current_input(&self.term))
            .map(|(text, _, _)| text)
    }

    fn prompt_input(&self) -> Option<zet_suggest::PromptInput> {
        let (text, col, abs) = self
            .recorder
            .as_ref()
            .and_then(|rec| rec.current_input(&self.term))?;
        let (cx, cy) = self.term.cursor();
        Some(zet_suggest::PromptInput {
            text,
            prompt_col: col,
            prompt_row: abs,
            cursor: (cx, self.term.scrollback_len() + cy),
        })
    }

    /// Replace the current line with `cmd` (Ctrl+U then type).
    fn accept_line(&mut self, cmd: &str) {
        self.ghost.reset();
        if let Some(s) = &mut self.session {
            let _ = s.write_all(&[0x15]);
            let _ = s.write_all(cmd.as_bytes());
        }
    }

    fn retitle(&self) {
        if let Some(w) = &self.window {
            let nl = if self.nl_mode { " (nl)" } else { "" };
            w.set_title(&format!("ZET — {}{nl}", self.shell_name));
        }
    }

    /// Kick off an NL-to-command request on a thread; the answer swaps the
    /// line when it lands (polled in update_ghost). Returns false with no
    /// model configured.
    fn start_nl_request(&mut self, nl: String) -> bool {
        let spec = match &self.inline_spec {
            Some(s) => s.clone(),
            None => {
                self.nl_mode = false;
                self.retitle();
                self.ghost
                    .notify(nl, "(set ZET_INLINE_SPEC for NL)".to_string());
                return false;
            }
        };
        let (tx, rx) = mpsc::channel();
        self.nl_rx = Some(rx);
        std::thread::spawn(move || {
            let answer = build_suggester(&spec).and_then(|m| m.suggest_nl(&nl));
            let _ = tx.send(answer);
        });
        true
    }
}

/// Build a concrete (Send) model suggester from a spec string.
fn build_suggester(
    spec: &str,
) -> Option<zet_suggest::ModelSuggester<zet_models::openai::OpenAICompat>> {
    use zet_models::keys::{resolve_provider, EnvKeychain, Keychain, SystemKeychain};
    use zet_models::Provider;
    let env = EnvKeychain;
    let sys = SystemKeychain;
    let chains: [&dyn Keychain; 2] = [&env, &sys];
    let provider = resolve_provider(spec, &chains).ok()?;
    let (base_url, api_key, model) = match provider {
        Provider::OpenAICompat {
            base_url,
            api_key,
            model,
        } => (base_url, api_key, model),
        Provider::Local { endpoint, model } => (endpoint, String::new(), model),
    };
    Some(zet_suggest::ModelSuggester::new(
        zet_models::openai::OpenAICompat::new(base_url, api_key, model),
    ))
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
        // Heartbeat re-armed every about_to_wait; this initial value only
        // covers the first idle gap (events and the PTY proxy wake sooner).
        event_loop.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + FRAME));
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // PTY reader thread has bytes: prompt frame, coalesced by the flag.
        self.needs_draw = true;
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Requesting unconditionally here would keep a redraw permanently
        // pending and the loop would never sleep — the original spin.
        if self.needs_draw || std::time::Instant::now() >= self.last_frame + FRAME {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.last_frame + FRAME));
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
                // Any key gets a prompt frame; the heartbeat covers the rest.
                self.needs_draw = true;
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
                // NL mode toggle: Ctrl+; would otherwise type ESC (0x1b).
                if is_nl_toggle(&event.logical_key, pressed, &self.mods) {
                    self.nl_mode = !self.nl_mode;
                    self.ghost.reset();
                    self.nl_rx = None;
                    self.retitle();
                    return;
                }
                let is_tab = pressed && matches!(&event.logical_key, Key::Named(NamedKey::Tab));
                let is_enter = pressed && matches!(&event.logical_key, Key::Named(NamedKey::Enter));
                let is_esc = pressed && matches!(&event.logical_key, Key::Named(NamedKey::Escape));
                // Tab accepts only a completable history ghost; notices
                // ("…", hints) fall through to the shell untouched.
                if is_tab {
                    if let Some(suffix) = self.ghost.accept() {
                        if let Some(s) = &mut self.session {
                            let _ = s.write_all(suffix.as_bytes());
                        }
                        return;
                    }
                }
                // Escape clears ghost / cancels NL; swallowed only when it did.
                if is_esc {
                    let was_nl = self.nl_mode || self.nl_rx.is_some();
                    let swallowed = self.ghost.cancel();
                    if was_nl {
                        self.ghost.dismiss_current();
                    }
                    self.nl_rx = None;
                    if self.nl_mode {
                        self.nl_mode = false;
                        self.retitle();
                    }
                    if swallowed || was_nl {
                        return;
                    }
                }
                // Enter in NL mode sends the line to the model, not the shell.
                if is_enter && self.nl_mode && self.nl_rx.is_none() {
                    let input = self.current_text().unwrap_or_default();
                    if !input.trim().is_empty() {
                        self.start_nl_request(input);
                    }
                    return;
                }
                // Enter while an NL answer is in flight: wait for it.
                if is_enter && self.nl_rx.is_some() {
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
                self.needs_draw = true;
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
                if self.dead {
                    event_loop.exit();
                    return;
                }
                if let (Some(r), Some(_)) = (&mut self.renderer, &self.window) {
                    r.draw_terminal(&self.term, self.cols, self.rows);
                    if let Some(g) = self.ghost.visible() {
                        let (cx, cy) = self.term.cursor();
                        r.draw_ghost(cy, cx, g, self.cols);
                    }
                    r.render();
                    self.last_frame = std::time::Instant::now();
                    self.needs_draw = false;
                }
                // No re-arm here: about_to_wait drives the heartbeat.
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
    let event_loop = EventLoop::new().unwrap();
    // Wake the loop the moment PTY bytes land: output renders instantly
    // instead of waiting for the heartbeat.
    let proxy = event_loop.create_proxy();
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
                    let _ = proxy.send_event(());
                }
                Err(_) => break,
            }
        }
    });

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
    // Second handle on the same store for instant history suggestions
    // (owned by the ghost controller; borrows would fight the
    // recorder's &mut pump). In-memory fallback: ghost degrades to
    // empty suggestions instead of disabling the shell.
    let ghost = zet_suggest::Ghost::new(
        zet_blocks::Store::open_default()
            .or_else(|_| zet_blocks::Store::open_in_memory())
            .expect("in-memory store"),
    );
    // NL model for Ctrl+; mode: spec like "local:qwen3-1.7b-q4" or
    // "openrouter:openai/gpt-4o-mini" (key from env/OS keychain).
    let inline_spec = std::env::var("ZET_INLINE_SPEC").ok();
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
        inline_spec,
        ghost,
        nl_mode: false,
        nl_rx: None,
        last_frame: std::time::Instant::now(),
        needs_draw: true,
    };
    if let Some(s) = &app.session {
        app.erase = s.erase_char();
    }
    event_loop.run_app(&mut app).unwrap();
}
