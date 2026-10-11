// Live shell: real PTY + grid + GPU, keyboard in, pixels out.
// Usage: cargo run -p zet-gpu --example shell
// This is the human smoke test (launcher points here).

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::Window;
use zet_gpu::input::{is_agent_toggle, is_nl_toggle, is_paste, paste_bytes, translate};
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
    goal_mode: bool,
    agent_busy: bool,
    agent_rx: Option<Receiver<Option<AgentDone>>>,
    exec_req_rx: Option<Receiver<String>>,
    exec_res_tx: Option<Sender<ExecRes>>,
    exec_pending: Option<PendingExec>,
    last_journal: Vec<zet_agent::JournalEntry>,
    last_frame: std::time::Instant,
    needs_draw: bool,
    perf_t0: Option<std::time::Instant>,
    perf_input: Option<std::time::Instant>,
    perf_frames: u64,
    cmd_t0: Option<std::time::Instant>,
    perf_blocks: usize,
}

/// One approved agent step, serviced by the main loop: typed visibly into
/// the PTY (Ctrl+U, command, Enter) so hooks record it as a normal block,
/// then completed from the recorded block (output + exit code).
struct PendingExec {
    cmd: String,
    typed: bool,
    blocks_before: usize,
    deadline: std::time::Instant,
}

type ExecRes = (Option<i32>, String);
type AgentDone = (String, Vec<zet_agent::JournalEntry>);

/// Executor that runs agent steps in the visible shell: sends the command
/// to the window (main loop types it) and blocks for the recorded result.
struct PtyExec {
    req: Sender<String>,
    res: Receiver<ExecRes>,
}

impl zet_agent::Executor for PtyExec {
    fn exec(&mut self, command: &str, _workdir: &std::path::Path) -> ExecRes {
        if self.req.send(command.to_string()).is_err() {
            return (None, "window gone".to_string());
        }
        self.res.recv().unwrap_or((None, "window gone".to_string()))
    }
}

/// Agent-mode approver: entering agent mode pre-consents T0-T2 (Ask);
/// T3 (ConfirmType) is never approved — the model sees a denial and adapts.
/// Every step is journaled; repos get pre-mutation checkpoints + undo.
struct AgentMode;

impl zet_agent::Approver for AgentMode {
    fn approve(&self, tier: zet_policy::Tier, _command: &str) -> bool {
        !matches!(tier, zet_policy::Tier::T3)
    }
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
        // Settle probe (ZET_PERF=1 only): Enter-to-block with a fully
        // drained channel. Parsed + recorded; pixels ≤1 frame behind
        // (the D chunk's proxy poke already requested the frame).
        if self.perf_t0.is_some() {
            if let Some(rec) = &self.recorder {
                if let Ok(blocks) = rec.blocks() {
                    if blocks.len() > self.perf_blocks {
                        self.perf_blocks = blocks.len();
                        if let Some(t0) = self.cmd_t0.take() {
                            eprintln!("perf: cmd_settle_ms={}", t0.elapsed().as_millis());
                        }
                    }
                }
            }
        }
        self.service_exec();
        self.poll_agent_done();
        if let Some(code) = self.shell_state.last_exit() {
            if let Some(w) = &self.window {
                let mode = if self.agent_busy {
                    " (agent…)"
                } else if self.goal_mode {
                    " (agent)"
                } else if self.nl_mode {
                    " (nl)"
                } else {
                    ""
                };
                w.set_title(&format!("ZET — {} (exit {code}){mode}", self.shell_name));
            }
        }
        self.update_ghost();
    }

    /// Type one pending agent step visibly, then complete it from the
    /// recorded block. Single-lane: the agent waits, input is locked, so
    /// the newest block past `blocks_before` is our step's.
    fn service_exec(&mut self) {
        if self.exec_pending.is_none() {
            if let Some(rx) = &self.exec_req_rx {
                if let Ok(cmd) = rx.try_recv() {
                    let before = self
                        .recorder
                        .as_ref()
                        .and_then(|r| r.blocks().ok())
                        .map(|b| b.len())
                        .unwrap_or(0);
                    self.exec_pending = Some(PendingExec {
                        cmd,
                        typed: false,
                        blocks_before: before,
                        deadline: std::time::Instant::now() + std::time::Duration::from_secs(120),
                    });
                }
            }
        }
        let res: Option<ExecRes> = {
            let Some(st) = &mut self.exec_pending else {
                return;
            };
            if !st.typed {
                if let Some(s) = &mut self.session {
                    let _ = s.write_all(&[0x15]);
                    let _ = s.write_all(st.cmd.as_bytes());
                    let _ = s.write_all(b"\r");
                }
                st.typed = true;
                // Agent-typed steps record blocks too: void any human stamp
                // so their settle never prints against it.
                self.cmd_t0 = None;
                return;
            }
            if self.dead {
                Some((None, "shell exited".to_string()))
            } else if self.recorder.is_none() {
                Some((None, "block recorder unavailable".to_string()))
            } else {
                let blocks = self
                    .recorder
                    .as_ref()
                    .and_then(|r| r.blocks().ok())
                    .unwrap_or_default();
                if blocks.len() > st.blocks_before {
                    blocks.last().map(|b| {
                        (
                            Some(b.exit_code),
                            format!("{}\n[exit {}]", b.output, b.exit_code),
                        )
                    })
                } else if std::time::Instant::now() > st.deadline {
                    Some((None, "exec timed out waiting for the prompt".to_string()))
                } else {
                    None
                }
            }
        };
        if let Some(res) = res {
            if let Some(tx) = &self.exec_res_tx {
                let _ = tx.send(res);
            }
            self.exec_pending = None;
        }
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
            let mode = if self.agent_busy {
                " (agent…)"
            } else if self.goal_mode {
                " (agent)"
            } else if self.nl_mode {
                " (nl)"
            } else {
                ""
            };
            w.set_title(&format!("ZET — {}{mode}", self.shell_name));
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

    /// Start an agent turn on a thread: each approved step is typed
    /// visibly into the PTY and completed from its recorded block.
    /// Entering agent mode pre-consents T0-T2; T3 is never approved.
    fn start_agent(&mut self, goal: String) {
        let spec = match &self.inline_spec {
            Some(s) => s.clone(),
            None => {
                self.goal_mode = false;
                self.retitle();
                self.ghost
                    .notify(goal, "(set ZET_INLINE_SPEC for agent)".to_string());
                return;
            }
        };
        let cwd = shell_cwd(&self.session);
        let in_repo = zet_agent::detect_repo(std::path::Path::new(&cwd));
        let (req_tx, req_rx) = mpsc::channel::<String>();
        let (res_tx, res_rx) = mpsc::channel::<ExecRes>();
        let (done_tx, done_rx) = mpsc::channel::<Option<AgentDone>>();
        self.exec_req_rx = Some(req_rx);
        self.exec_res_tx = Some(res_tx);
        self.agent_rx = Some(done_rx);
        self.goal_mode = false;
        self.agent_busy = true;
        self.retitle();
        std::thread::spawn(move || {
            let done = build_client(&spec).map(|client| {
                let mut agent = zet_agent::Agent::new(client, AgentMode, in_repo, 8)
                    .with_workdir(cwd.clone().into())
                    .with_executor(PtyExec {
                        req: req_tx,
                        res: res_rx,
                    });
                let summary = agent
                    .run(&goal)
                    .unwrap_or_else(|e| format!("agent error: {e}"));
                (summary, agent.journal)
            });
            let _ = done_tx.send(done);
        });
    }

    /// Collect a finished agent turn: journal kept, summary as a sticky
    /// notice on the fresh prompt, modes cleared.
    fn poll_agent_done(&mut self) {
        let Some(rx) = &self.agent_rx else {
            return;
        };
        let Ok(done) = rx.try_recv() else {
            return;
        };
        self.agent_rx = None;
        self.agent_busy = false;
        self.exec_req_rx = None;
        self.exec_res_tx = None;
        self.exec_pending = None;
        self.retitle();
        let (input, text) = match done {
            Some((summary, journal)) => {
                // Best effort: the turn survives the window via `zet journal`.
                let _ = zet_agent::append_journal(&zet_agent::journal_path_default(), &journal);
                self.last_journal = journal;
                let first: String = summary
                    .lines()
                    .next()
                    .unwrap_or("done")
                    .chars()
                    .take(120)
                    .collect();
                (
                    self.current_text().unwrap_or_default(),
                    format!("agent: {first}"),
                )
            }
            None => (
                self.current_text().unwrap_or_default(),
                "(agent: no model — check spec/key)".to_string(),
            ),
        };
        self.ghost.notify(input, text);
    }

    /// Perf probe (ZET_PERF=1 only): first-frame age, key-to-render
    /// latency, frame counter. Zero cost when disabled (early return).
    fn note_frame(&mut self) {
        let Some(t0) = self.perf_t0 else {
            return;
        };
        self.perf_frames += 1;
        if self.perf_frames == 1 {
            eprintln!("perf: first_frame_ms={}", t0.elapsed().as_millis());
        }
        if let Some(tin) = self.perf_input.take() {
            eprintln!("perf: input_latency_us={}", tin.elapsed().as_micros());
        }
        if self.perf_frames.is_multiple_of(120) {
            eprintln!("perf: frames={}", self.perf_frames);
        }
    }
}

/// Build a concrete (Send) model suggester from a spec string.
fn build_suggester(
    spec: &str,
) -> Option<zet_suggest::ModelSuggester<zet_models::openai::OpenAICompat>> {
    build_client(spec).map(zet_suggest::ModelSuggester::new)
}

/// Concrete model client for agent turns (Send; built per request).
fn build_client(spec: &str) -> Option<zet_models::openai::OpenAICompat> {
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
    Some(zet_models::openai::OpenAICompat::new(
        base_url, api_key, model,
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
                if pressed && self.perf_t0.is_some() {
                    self.perf_input = Some(std::time::Instant::now());
                }
                // Agent running: input locked (its steps own the line).
                // Ctrl+C still reaches the shell: the emergency brake that
                // cancels the agent's running command (D fires, step ends).
                if self.agent_busy {
                    if let Some(bytes) = translate(
                        &event.logical_key,
                        event.text.as_deref(),
                        pressed,
                        &self.mods,
                    ) {
                        if bytes == [0x03] {
                            if let Some(s) = &mut self.session {
                                let _ = s.write_all(&bytes);
                            }
                        }
                    }
                    return;
                }
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
                    self.goal_mode = false;
                    self.ghost.reset();
                    self.nl_rx = None;
                    self.retitle();
                    return;
                }
                // Agent goal mode toggle: Ctrl+G would otherwise type BEL.
                if is_agent_toggle(&event.logical_key, pressed, &self.mods) {
                    self.goal_mode = !self.goal_mode;
                    self.nl_mode = false;
                    self.ghost.reset();
                    self.nl_rx = None;
                    self.retitle();
                    return;
                }
                let is_tab = pressed && matches!(&event.logical_key, Key::Named(NamedKey::Tab));
                let is_enter = pressed && matches!(&event.logical_key, Key::Named(NamedKey::Enter));
                let is_esc = pressed && matches!(&event.logical_key, Key::Named(NamedKey::Escape));
                // F12: one-key undo of the last checkpointed agent step.
                // (While busy the early gate above already swallowed this.)
                if pressed && matches!(&event.logical_key, Key::Named(NamedKey::F12)) {
                    let input = self.current_text().unwrap_or_default();
                    let cp = self
                        .last_journal
                        .iter()
                        .rev()
                        .find_map(|e| e.checkpoint.clone());
                    match cp {
                        None => {
                            self.ghost.notify(input, "(nothing to undo)".to_string());
                        }
                        Some(cp) => {
                            let cwd = shell_cwd(&self.session);
                            // Wrong-repo use fails closed: reset --hard with
                            // an unknown SHA errors instead of touching state.
                            match zet_agent::undo_checkpoint(std::path::Path::new(&cwd), &Some(cp))
                            {
                                Ok(()) => {
                                    self.ghost.notify(input, "(undone)".to_string());
                                }
                                Err(e) => {
                                    self.ghost.notify(input, format!("(undo failed: {e})"));
                                }
                            }
                        }
                    }
                    return;
                }
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
                // Enter in goal mode starts an agent turn on the line.
                if is_enter && self.goal_mode {
                    let input = self.current_text().unwrap_or_default();
                    if !input.trim().is_empty() {
                        self.start_agent(input);
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
                    // Settle-probe stamps (ZET_PERF=1 only): Enter on a
                    // non-empty line starts the clock; Ctrl+C voids it
                    // (aborted typing records no block). NL/goal/agent
                    // paths return before here, so this is human shell
                    // commands only.
                    if self.perf_t0.is_some() {
                        if bytes.as_slice() == b"\r" {
                            if self.current_text().is_some_and(|t| !t.trim().is_empty()) {
                                self.cmd_t0 = Some(std::time::Instant::now());
                            }
                        } else if bytes.as_slice() == b"\x03" {
                            self.cmd_t0 = None;
                        }
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
                    self.note_frame();
                }
                // No re-arm here: about_to_wait drives the heartbeat.
            }
            _ => {}
        }
    }
}

fn main() {
    zet_gpu::perf_mark("main");
    let (prog, hook) = shell_program();
    let shell_name = prog.clone();
    // Guarantee a sane TERM even under GUI launchers.
    let mut session = Session::spawn("env", &["TERM=xterm-256color", &prog, "-i"], 80, 24).unwrap();
    zet_gpu::perf_mark("shell_spawn");
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
    let perf_t0 = std::env::var("ZET_PERF")
        .ok()
        .map(|_| std::time::Instant::now());
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
        goal_mode: false,
        agent_busy: false,
        agent_rx: None,
        exec_req_rx: None,
        exec_res_tx: None,
        exec_pending: None,
        last_journal: Vec::new(),
        last_frame: std::time::Instant::now(),
        needs_draw: true,
        perf_t0,
        perf_input: None,
        perf_frames: 0,
        cmd_t0: None,
        perf_blocks: 0,
    };
    if let Some(s) = &app.session {
        app.erase = s.erase_char();
    }
    event_loop.run_app(&mut app).unwrap();
}
