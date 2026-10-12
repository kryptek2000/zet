// Live shell: real PTY + grid + GPU, keyboard in, pixels out.
// Usage: cargo run -p zet-gpu --example shell
// This is the human smoke test (launcher points here).

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::Window;
use zet_gpu::input::{is_agent_toggle, is_copy, is_paste, paste_bytes, translate};
use zet_gpu::selection::{pix_to_cell, Selection};
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
    env_spec: Option<String>,
    startup_spec: Option<String>,
    ghost: zet_suggest::Ghost,
    nl_rx: Option<Receiver<Result<Option<String>, String>>>,
    /// The in-flight NL request came from the `zet` wake word: its
    /// answer runs (unless T3), not just previews.
    nl_wake: bool,
    /// Commands that run without translation (PATH at startup +
    /// shell-reported aliases/functions). Unknown first words route
    /// to the model instead — no modes, no chords.
    known: zet_pty::KnownCommands,
    /// Mouse selection (absolute cells): drag to select, Ctrl+Shift+C
    /// or right-click to copy. Typing clears it.
    selection: Option<Selection>,
    selecting: bool,
    mouse_px: Option<(f64, f64)>,
    was_alt: bool,
    /// Computed fix for the last failure, offered as ghost preview once
    /// the prompt is fresh. Tab takes it, Esc/typing clears it.
    pending_fix: Option<String>,
    goal_mode: bool,
    agent_busy: bool,
    agent_rx: Option<Receiver<Option<AgentDone>>>,
    exec_req_rx: Option<Receiver<String>>,
    exec_res_tx: Option<Sender<ExecRes>>,
    exec_pending: Option<PendingExec>,
    last_journal: Vec<zet_agent::JournalEntry>,
    undo_rx: Option<Receiver<Result<(), String>>>,
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
/// Syntax-gated like NL answers: broken commands fail fast with exit 2
/// (bash's own syntax-error code) instead of stranding a continuation.
struct PtyExec {
    req: Sender<String>,
    res: Receiver<ExecRes>,
    shell: String,
}

impl zet_agent::Executor for PtyExec {
    fn check(&self, command: &str) -> Option<String> {
        if zet_pty::syntax_ok(&self.shell, command) {
            None
        } else {
            Some(format!("syntax error, not executed: {command}"))
        }
    }

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
            // Shell-reported aliases/functions feed the router.
            self.known.insert_names(rec.drain_cmd_names());
            // Failures become did-you-mean offers (preview only).
            if let Some((cmd, cwd, output, exit)) = rec.drain_failed() {
                let fix = zet_pty::suggest_fix(&cmd, &cwd, &output, exit, &self.known);
                if fix.is_some() {
                    self.pending_fix = fix;
                }
            }
        }
        // Alt switch swaps the visible grid: any selection addresses
        // stale rows, so drop it (both directions).
        let alt = self.term.alt_active();
        if alt != self.was_alt {
            self.selection = None;
            self.selecting = false;
            self.was_alt = alt;
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
        self.poll_undo_done();
        self.poll_fix_offer();
        if let Some(code) = self.shell_state.last_exit() {
            if let Some(w) = &self.window {
                let mode = if self.agent_busy {
                    " (agent…)"
                } else if self.goal_mode {
                    " (agent)"
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

    /// Collect a finished F12 undo: sticky notice on the current line.
    /// A dead worker (panicked thread) clears the channel with a failure
    /// notice instead of locking F12 forever.
    fn poll_undo_done(&mut self) {
        let Some(rx) = &self.undo_rx else {
            return;
        };
        match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => (),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.undo_rx = None;
                let input = self.current_text().unwrap_or_default();
                self.ghost
                    .notify(input, "(undo failed: worker gone)".to_string());
            }
            Ok(res) => {
                self.undo_rx = None;
                let input = self.current_text().unwrap_or_default();
                match res {
                    Ok(()) => self.ghost.notify(input, "(undone)".to_string()),
                    Err(e) => self.ghost.notify(input, format!("(undo failed: {e})")),
                }
            }
        }
    }

    /// Refresh the visible ghost suffix from current input. History only
    /// (sync, sub-ms); the model runs solely on NL Enter, in a thread.
    fn update_ghost(&mut self) {
        // NL answer landed: swap the NL line for the command (the preview).
        if let Some(rx) = &self.nl_rx {
            match rx.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.ghost.waiting();
                    return;
                }
                // Dead worker: drop back to history instead of "…" forever.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.nl_rx = None;
                    self.nl_wake = false;
                }
                // Model failures notice LOUD (rate limits, net, auth) —
                // never dissolve into `(no command)` again.
                Ok(Err(e)) => {
                    self.nl_rx = None;
                    self.nl_wake = false;
                    if let Some(input) = self.current_text() {
                        let short: String = e.chars().take(100).collect();
                        self.ghost.notify(input, format!("(model error: {short})"));
                    }
                }
                Ok(Ok(answer)) => {
                    self.nl_rx = None;
                    if let Some(cmd) = answer {
                        // Conversation, not a command: answer it on the
                        // prompt line instead of swapping. `hello` gets a
                        // reply, not an exit 127.
                        if let Some(chat) = zet_suggest::chat_reply(&cmd) {
                            if let Some(input) = self.current_text() {
                                self.ghost.notify(input, format!("agent: {chat}"));
                            }
                        // Never strand the shell in a continuation prompt:
                        // syntactically broken replies notice instead of swap.
                        } else if zet_pty::syntax_ok(&self.shell_name, &cmd) {
                            self.accept_line(&cmd);
                            // Wake-word orders (`zet update linux`) run:
                            // explicit instruction, T3 never auto-passes.
                            if self.nl_wake && zet_policy::classify(&cmd) != zet_policy::Tier::T3 {
                                if let Some(s) = &mut self.session {
                                    let _ = s.write_all(b"\r");
                                }
                            } else if self.nl_wake {
                                self.ghost.notify(
                                    cmd.clone(),
                                    "(review: critical command — Enter to run)".to_string(),
                                );
                            }
                        } else if let Some(input) = self.current_text() {
                            let short: String = cmd.chars().take(60).collect();
                            self.ghost.notify(input, format!("(invalid: {short})"));
                        }
                    } else if let Some(input) = self.current_text() {
                        self.ghost.notify(input, "(no command)".to_string());
                    }
                    self.nl_wake = false;
                    self.retitle();
                }
            }
        }
        let input = self.prompt_input();
        self.ghost.update(input);
    }

    /// Offer a pending did-you-mean fix once the prompt is fresh.
    /// Called every frame: waits for the empty line, then previews.
    fn poll_fix_offer(&mut self) {
        if self.pending_fix.is_some() && self.current_text().is_some_and(|t| t.is_empty()) {
            if let Some(fix) = self.pending_fix.take() {
                self.ghost.preview(fix);
            }
        }
    }

    fn current_text(&self) -> Option<String> {
        self.recorder
            .as_ref()
            .and_then(|rec| rec.current_input(&self.term))
            .map(|(text, _, _)| text)
    }

    /// Cell under the last known mouse position (absolute rows).
    fn mouse_cell(&self) -> Option<zet_gpu::selection::CellPos> {
        let (x, y) = self.mouse_px?;
        let r = self.renderer.as_ref()?;
        let (cw, ch) = r.cell_px();
        Some(pix_to_cell(
            x,
            y,
            cw,
            ch,
            self.cols,
            self.rows,
            self.term.abs_base(),
        ))
    }

    /// Copy the mouse selection to the clipboard (no-op when empty).
    fn copy_selection(&self) {
        let Some(sel) = &self.selection else {
            return;
        };
        let text = sel.text(&self.term);
        if text.is_empty() {
            return;
        }
        if let Ok(mut clip) = arboard::Clipboard::new() {
            let _ = clip.set_text(text);
        }
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
            } else {
                ""
            };
            w.set_title(&format!("ZET — {}{mode}", self.shell_name));
        }
    }

    /// Model spec for this NL/agent request: explicit env (launch config)
    /// first, then the persisted `zet inline` choice (re-read every time
    /// so no restart is needed), then the startup value, then a fresh
    /// local probe in case the server started since launch.
    fn resolve_spec(&self) -> Option<String> {
        let path = zet_models::keys::inline_spec_path();
        if let Some(spec) = zet_models::keys::resolve_inline_spec(
            self.env_spec.clone(),
            &path,
            self.startup_spec.clone(),
        ) {
            return Some(spec);
        }
        zet_models::keys::autodetect_local().inspect(|s| eprintln!("zet: inline model {s}"))
    }

    /// Auto-route: known first words run instantly; anything else goes
    /// to the model (translate → preview). Empty/assignment-only lines
    /// run (they're shell business, not language). `zet` itself always
    /// runs: real subcommands execute, the wake-word case is peeled off
    /// earlier by zet_fallback, and anything left is CLI business.
    fn route_direct(&self, line: &str) -> bool {
        match zet_pty::first_word(line) {
            None => true,
            Some(w) => w == "zet" || self.known.contains(&w) || zet_pty::is_shell_word(&w),
        }
    }

    /// Specs fail LOUD, not silent: a bad/stale spec notices with the
    /// reason (spec text + provider names only — never key material)
    /// instead of dissolving into `(no command)`.
    fn check_spec(spec: &str) -> Result<(), String> {
        with_keychains(|chains| {
            zet_models::keys::resolve_provider(spec, chains)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }

    /// Kick off an NL-to-command request on a thread; the answer swaps the
    /// line when it lands (polled in update_ghost). Wake-word requests
    /// (`wake=true`) run their answer instead of previewing it (unless
    /// T3). Returns false with no model configured.
    fn start_nl_request(&mut self, nl: String, wake: bool) -> bool {
        let spec = match self.resolve_spec() {
            Some(s) => s,
            None => {
                self.ghost.notify(
                    nl,
                    "(no model: `zet inline <spec>` or ZET_INLINE_SPEC)".to_string(),
                );
                return false;
            }
        };
        if let Err(e) = Self::check_spec(&spec) {
            self.ghost.notify(nl, format!("({e})"));
            return false;
        }
        let (tx, rx) = mpsc::channel();
        self.nl_rx = Some(rx);
        self.nl_wake = wake;
        std::thread::spawn(move || {
            // check_spec passed, so the client builds; a race (key
            // deleted between) reports instead of going silent.
            let answer: Result<Option<String>, String> = match build_suggester(&spec) {
                None => Err("model unavailable".to_string()),
                Some(m) => m.suggest_nl(&nl).map_err(|e| e.to_string()),
            };
            let _ = tx.send(answer);
        });
        true
    }

    /// Start an agent turn on a thread: each approved step is typed
    /// visibly into the PTY and completed from its recorded block.
    /// Entering agent mode pre-consents T0-T2; T3 is never approved.
    fn start_agent(&mut self, goal: String) {
        let spec = match self.resolve_spec() {
            Some(s) => s,
            None => {
                self.goal_mode = false;
                self.retitle();
                self.ghost.notify(
                    goal,
                    "(no model: `zet inline <spec>` or ZET_INLINE_SPEC)".to_string(),
                );
                return;
            }
        };
        if let Err(e) = Self::check_spec(&spec) {
            self.goal_mode = false;
            self.retitle();
            self.ghost.notify(goal, format!("({e})"));
            return;
        }
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
        let shell_name = self.shell_name.clone();
        std::thread::spawn(move || {
            let done = build_client(&spec).map(|client| {
                let mut agent = zet_agent::Agent::new(client, AgentMode, in_repo, 8)
                    .with_workdir(cwd.clone().into())
                    .with_executor(PtyExec {
                        req: req_tx,
                        res: res_rx,
                        shell: shell_name,
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
    /// notice on the fresh prompt, modes cleared. A dead worker unlocks
    /// input with a failure notice instead of holding the lock forever.
    fn poll_agent_done(&mut self) {
        let Some(rx) = &self.agent_rx else {
            return;
        };
        match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => (),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.agent_rx = None;
                self.agent_busy = false;
                self.exec_req_rx = None;
                self.exec_res_tx = None;
                self.exec_pending = None;
                self.retitle();
                let input = self.current_text().unwrap_or_default();
                self.ghost
                    .notify(input, "(agent failed: worker gone)".to_string());
            }
            Ok(done) => {
                self.agent_rx = None;
                self.agent_busy = false;
                self.exec_req_rx = None;
                self.exec_res_tx = None;
                self.exec_pending = None;
                self.retitle();
                let (input, text) = match done {
                    Some((summary, journal)) => {
                        // Best effort: the turn survives the window via `zet journal`.
                        let _ =
                            zet_agent::append_journal(&zet_agent::journal_path_default(), &journal);
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
        }
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

/// Keychain search order, shared by spec validation and client builds
/// so the two can never drift apart (env first, OS keychain second).
fn with_keychains<T>(f: impl FnOnce(&[&dyn zet_models::keys::Keychain]) -> T) -> T {
    use zet_models::keys::{EnvKeychain, SystemKeychain};
    let env = EnvKeychain;
    let sys = SystemKeychain;
    let chains: [&dyn zet_models::keys::Keychain; 2] = [&env, &sys];
    f(&chains)
}

/// Build a concrete (Send) model suggester from a spec string.
fn build_suggester(spec: &str) -> Option<zet_suggest::ModelSuggester<zet_models::Client>> {
    build_client(spec).map(zet_suggest::ModelSuggester::new)
}

/// Concrete model client for agent turns (Send; built per request).
/// The opencode: provider yields the Go gateway client (session
/// headers + chat/responses fallback); everything else is OpenAI-shape.
fn build_client(spec: &str) -> Option<zet_models::Client> {
    use zet_models::Provider;
    let provider = with_keychains(|chains| zet_models::keys::resolve_provider(spec, chains).ok())?;
    Some(match provider {
        Provider::Go { api_key, model } => zet_models::Client::Go(zet_models::go::GoClient::new(
            zet_models::go::GO_BASE_URL.to_string(),
            api_key,
            model,
        )),
        Provider::OpenAICompat {
            base_url,
            api_key,
            model,
        } => zet_models::Client::OpenAI(zet_models::openai::OpenAICompat::new(
            base_url, api_key, model,
        )),
        Provider::Local { endpoint, model } => zet_models::Client::OpenAI(
            zet_models::openai::OpenAICompat::new(endpoint, String::new(), model),
        ),
    })
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
                // Copy is local, not shell input: reading the grid never
                // disturbs the turn, so it stays available while busy.
                // Ctrl+C still reaches the shell: the emergency brake that
                // cancels the agent's running command (D fires, step ends).
                if self.agent_busy {
                    if is_copy(&event.logical_key, pressed, &self.mods) {
                        self.copy_selection();
                        return;
                    }
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
                // Copy: Ctrl+Shift+C copies the mouse selection (never ETX).
                if is_copy(&event.logical_key, pressed, &self.mods) {
                    self.copy_selection();
                    return;
                }
                // Agent goal mode toggle: Ctrl+G would otherwise type BEL.
                if is_agent_toggle(&event.logical_key, pressed, &self.mods) {
                    self.goal_mode = !self.goal_mode;
                    self.ghost.reset();
                    self.nl_rx = None;
                    self.nl_wake = false;
                    self.retitle();
                    return;
                }
                let is_tab = pressed && matches!(&event.logical_key, Key::Named(NamedKey::Tab));
                let is_enter = pressed && matches!(&event.logical_key, Key::Named(NamedKey::Enter));
                let is_esc = pressed && matches!(&event.logical_key, Key::Named(NamedKey::Escape));
                // F12: one-key undo of the last checkpointed agent step.
                // (While busy the early gate above already swallowed this.)
                // Runs on a thread: git in a giant repo must not hitch frames.
                if pressed && matches!(&event.logical_key, Key::Named(NamedKey::F12)) {
                    if self.undo_rx.is_some() {
                        return;
                    }
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
                            let (tx, rx) = mpsc::channel();
                            self.undo_rx = Some(rx);
                            std::thread::spawn(move || {
                                // Wrong-repo use fails closed: reset --hard
                                // with an unknown SHA errors, touching nothing.
                                let res = zet_agent::undo_checkpoint(
                                    std::path::Path::new(&cwd),
                                    &Some(cp),
                                )
                                .map_err(|e| e.to_string());
                                let _ = tx.send(res);
                            });
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
                // Escape clears ghost / cancels an in-flight answer / exits
                // goal mode; swallowed only when it did something.
                if is_esc {
                    let was_pending = self.nl_rx.is_some();
                    let was_goal = self.goal_mode;
                    let had_fix = self.pending_fix.take().is_some();
                    let swallowed = self.ghost.cancel();
                    if was_pending || was_goal {
                        self.ghost.dismiss_current();
                    }
                    self.nl_rx = None;
                    self.nl_wake = false;
                    if self.goal_mode {
                        self.goal_mode = false;
                        self.retitle();
                    }
                    if swallowed || was_pending || was_goal || had_fix {
                        return;
                    }
                }
                // Enter in goal mode starts an agent turn on the line.
                if is_enter && self.goal_mode {
                    let input = self.current_text().unwrap_or_default();
                    if !input.trim().is_empty() {
                        self.start_agent(input);
                    }
                    return;
                }
                // Modeless routing: `zet <words>` translates the rest;
                // known commands run; unknown first words translate
                // (preview swaps in, never runs blind). The line stays
                // put when there's no model (notice shown).
                if is_enter && self.nl_rx.is_none() {
                    let input = self.current_text().unwrap_or_default();
                    if !input.trim().is_empty() {
                        if let Some(rest) = zet_pty::zet_fallback(&input) {
                            // Wake word: an order, not a question.
                            self.start_nl_request(rest, true);
                            return;
                        }
                        if !self.route_direct(&input) {
                            self.start_nl_request(input, false);
                            return;
                        }
                    }
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
                    // Typing clears the mouse selection.
                    if pressed {
                        self.selection = None;
                    }
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
                    r.draw_terminal(&self.term, self.cols, self.rows, self.selection);
                    if let Some(g) = self.ghost.visible() {
                        let (cx, cy) = self.term.cursor();
                        r.draw_ghost(cy, cx, g, self.cols, self.rows);
                    }
                    r.render();
                    self.last_frame = std::time::Instant::now();
                    self.needs_draw = false;
                    self.note_frame();
                }
                // No re-arm here: about_to_wait drives the heartbeat.
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.needs_draw = true;
                match (button, state) {
                    // Left-drag selects; click without drag clears.
                    (MouseButton::Left, ElementState::Pressed) => {
                        if let Some(cell) = self.mouse_cell() {
                            self.selecting = true;
                            self.selection =
                                Some(Selection::new((cell.col, cell.row), (cell.col, cell.row)));
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        self.selecting = false;
                        if self.selection.is_some_and(|s| s.is_empty()) {
                            self.selection = None;
                        }
                    }
                    // Right-click copies the selection (terminal convention).
                    (MouseButton::Right, ElementState::Pressed) => {
                        self.copy_selection();
                    }
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_px = Some((position.x, position.y));
                if self.selecting {
                    if let Some(cell) = self.mouse_cell() {
                        if let Some(sel) = &mut self.selection {
                            sel.active = zet_gpu::selection::CellPos {
                                col: cell.col,
                                row: cell.row,
                            };
                            self.needs_draw = true;
                        }
                    }
                }
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
    // Model for plain-language input: explicit ZET_INLINE_SPEC (launch config)
    // wins; else the persisted `zet inline` choice re-read per request;
    // else zero-config local when ollama serves a preferred model.
    let env_spec = std::env::var("ZET_INLINE_SPEC").ok();
    let startup_spec = match &env_spec {
        Some(_) => None,
        None => {
            zet_models::keys::autodetect_local().inspect(|s| eprintln!("zet: inline model {s}"))
        }
    };
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
        env_spec,
        startup_spec,
        ghost,
        known: zet_pty::KnownCommands::from_env(),
        selection: None,
        selecting: false,
        mouse_px: None,
        was_alt: false,
        pending_fix: None,
        nl_rx: None,
        nl_wake: false,
        goal_mode: false,
        agent_busy: false,
        agent_rx: None,
        exec_req_rx: None,
        exec_res_tx: None,
        exec_pending: None,
        last_journal: Vec::new(),
        undo_rx: None,
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
