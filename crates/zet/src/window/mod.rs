//! The terminal window: real PTY + grid + GPU, keyboard in, pixels out.
//!
//! This is the product. It lives in the `zet` binary crate (the
//! composition root) so `zet-gpu` stays a pure renderer with no edge
//! to the agent/model stack. `run(goal)` opens the window; `Some(goal)`
//! starts that agent turn once the shell reaches its first prompt.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::Window;
use zet_gpu::input::{
    is_agent_toggle, is_close_tab, is_copy, is_cycle_tab, is_inspector_toggle, is_new_tab,
    is_paste, is_rail_toggle, is_search, is_select_all, translate,
};
use zet_gpu::selection::Selection;
use zet_gpu::Renderer;
use zet_pty::shell::ShellState;
use zet_pty::Session;
use zet_term::Terminal;

pub mod layout;
pub mod panels;

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

/// The lines typed into a fresh shell, in order: the session id the
/// shell exports, then the shell integration hook.
///
/// The session id travels by environment variable: the shell exports
/// it, so every child — `zet blocks`, `zet replay`, `zet journal` —
/// names this session without anyone having to pass it on the command
/// line. Typed into the PTY like the hook line below, so no zet-pty
/// change is needed.
fn setup_lines(session_id: &str, hook: Option<&str>) -> String {
    let mut out = format!("export ZET_SESSION={session_id}\n");
    if let Some(hook) = hook {
        out.push_str(&format!("source {hook}\nclear\n"));
    }
    out
}

/// One terminal tab: everything bound to a single shell session.
/// The window holds a Vec<Tab> + active index; shared chrome
/// (renderer, key/mouse state, model specs, perf) stays on App.
struct Tab {
    session: Option<Session>,
    term: Terminal,
    shell_state: ShellState,
    session_id: u64,
    rx: Option<Receiver<Vec<u8>>>,
    dead: bool,
    erase: u8,
    recorder: Option<zet_blocks::Recorder>,
    ghost: zet_suggest::Ghost,
    selection: Option<Selection>,
    selecting: bool,
    pending_fix: Option<String>,
    nl_rx: Option<Receiver<Result<Option<String>, String>>>,
    nl_wake: bool,
    goal_mode: bool,
    agent_busy: bool,
    agent_rx: Option<Receiver<Option<AgentDone>>>,
    plan_rx: Option<Receiver<PlanDraft>>,
    pending_plan: Option<(String, Vec<String>)>,
    exec_req_rx: Option<Receiver<String>>,
    exec_res_tx: Option<Sender<ExecRes>>,
    exec_pending: Option<PendingExec>,
    last_journal: Vec<zet_agent::JournalEntry>,
    undo_rx: Option<Receiver<Result<(), String>>>,
    was_alt: bool,
    search: SearchState,
}

/// Scrollback find state (per tab): the query, absolute-row matches,
/// and the current hit. Typing goes to the query, never the PTY.
struct SearchState {
    on: bool,
    query: String,
    matches: Vec<(usize, usize, usize)>,
    current: usize,
}

impl Tab {
    /// Spawn a shell + reader thread + recorder: the per-tab pieces
    /// of what main() built inline. `wake` nudges the event loop when
    /// PTY bytes land so output renders instantly.
    fn spawn(prog: &str, hook: Option<String>, wake: EventLoopProxy<()>) -> Self {
        let mut session =
            Session::spawn("env", &["TERM=xterm-256color", prog, "-i"], 80, 24).unwrap();
        zet_gpu::perf_mark("shell_spawn");
        let terminal_session_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let session_id = format!(
            "{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            std::process::id()
        );
        let setup = setup_lines(&session_id, hook.as_deref());
        let _ = session.write_all(setup.as_bytes());
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
                        let _ = wake.send_event(());
                    }
                    Err(_) => break,
                }
            }
        });
        let recorder = zet_blocks::Store::open_default()
            .map(|store| zet_blocks::Recorder::new(store, &session_id))
            .ok();
        let ghost = zet_suggest::Ghost::new(
            zet_blocks::Store::open_default()
                .or_else(|_| zet_blocks::Store::open_in_memory())
                .expect("in-memory store"),
        );
        let erase = session.erase_char();
        Tab {
            session: Some(session),
            term: Terminal::new(80, 24),
            shell_state: ShellState::new(),
            session_id: terminal_session_id,
            rx: Some(rx),
            dead: false,
            erase,
            recorder,
            ghost,
            selection: None,
            selecting: false,
            pending_fix: None,
            nl_rx: None,
            nl_wake: false,
            goal_mode: false,
            agent_busy: false,
            agent_rx: None,
            plan_rx: None,
            pending_plan: None,
            exec_req_rx: None,
            exec_res_tx: None,
            exec_pending: None,
            last_journal: zet_agent::read_journal(&zet_agent::journal_path_default())
                .map(|all| {
                    let start = all.len().saturating_sub(50);
                    all[start..].to_vec()
                })
                .unwrap_or_default(),
            undo_rx: None,
            was_alt: false,
            search: SearchState {
                on: false,
                query: String::new(),
                matches: Vec::new(),
                current: 0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Terminal,
    Rail,
    Inspector,
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    tabs: Vec<Tab>,
    active: usize,
    shell_name: String,
    /// Spawn recipe for new tabs: same shell + hooks, fresh PTY.
    prog: String,
    hook: Option<String>,
    wake: Option<EventLoopProxy<()>>,
    mods: ModifiersState,
    cols: usize,
    rows: usize,
    layout: crate::window::layout::Layout,
    focus: Focus,
    rail_sel: usize,
    chrome: panels::ChromeCache,
    inspector_sel: Option<usize>,
    inspector_cache: panels::InspectorCache,
    #[cfg(test)]
    pub chrome_query_count: usize,
    #[cfg(test)]
    pub chrome_build_count: usize,
    env_spec: Option<String>,
    startup_spec: Option<String>,
    /// Commands that run without translation (PATH at startup +
    /// shell-reported aliases/functions). Unknown first words route
    /// to the model instead — no modes, no chords.
    known: zet_pty::KnownCommands,
    mouse_px: Option<(f64, f64)>,
    last_frame: std::time::Instant,
    needs_draw: bool,
    perf_t0: Option<std::time::Instant>,
    perf_input: Option<std::time::Instant>,
    perf_frames: u64,
    cmd_t0: Option<std::time::Instant>,
    perf_blocks: usize,
    /// A turn handed to us on the command line (`zet "fix the test"`).
    /// It waits for the shell's first prompt, then starts that turn.
    /// `None` (the plain `zet` case) changes nothing.
    pending_goal: Option<String>,
}

/// One approved agent step, serviced by the main loop: typed visibly into
/// the PTY (Ctrl+U, command, Enter) so hooks record it as a normal block,
/// then completed from the recorded block (output + exit code).
struct PendingExec {
    cmd: String,
    typed: bool,
    blocks_before: usize,
    started: std::time::Instant,
    watch: zet_gpu::exec_watch::StepWatch,
}

type ExecRes = (Option<i32>, String);
type AgentDone = (String, Vec<zet_agent::JournalEntry>);
/// Planner loop phase one: the Enter-time goal plus its drafted steps
/// (or the draft failure). Lives on the plan channel into poll_plan.
type PlanDraft = (String, Result<Vec<String>, String>);

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
        // Every tab drains PTY bytes + records blocks (a background tab
        // with a full pipe would stall its shell otherwise). Title and
        // settle probe stay active-tab-only.
        let mut died_active = false;
        for i in 0..self.tabs.len() {
            if self.pump_tab(i) && i == self.active {
                died_active = true;
            }
        }
        if died_active {
            if let Some(w) = &self.window {
                w.set_title(&format!("ZET — {} (exited)", self.shell_name));
            }
        }
        // Settle probe (ZET_PERF=1 only): Enter-to-block with a fully
        // drained channel. Parsed + recorded; pixels ≤1 frame behind
        // (the D chunk's proxy poke already requested the frame).
        if self.perf_t0.is_some() {
            if let Some(rec) = &self.tabs[self.active].recorder {
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
        self.poll_plan();
        self.poll_undo_done();
        self.poll_fix_offer();
        if let Some(code) = self.tabs[self.active].shell_state.last_exit() {
            self.retitle_with_exit(code);
        }
        self.update_ghost();
    }

    /// Drain one tab's PTY + record its blocks. Returns true when its
    /// shell newly exited (caller titles only the active tab).
    fn pump_tab(&mut self, i: usize) -> bool {
        let mut died = false;
        // Drain PTY bytes first (borrow ends), then feed the grid.
        let chunks: Vec<Vec<u8>> = match &self.tabs[i].rx {
            Some(rx) => std::iter::from_fn(|| rx.try_recv().ok()).collect(),
            None => Vec::new(),
        };
        for chunk in chunks {
            self.tabs[i].term.feed(&chunk);
        }
        let responses = self.tabs[i].term.drain_responses();
        if !responses.is_empty() {
            if let Some(s) = &mut self.tabs[i].session {
                for response in responses {
                    let _ = s.write_all(&response);
                }
            }
        }
        if let Some(s) = &mut self.tabs[i].session {
            if let Ok(Some(_)) = s.try_exit() {
                died = true;
            }
        }
        if died {
            self.tabs[i].dead = true;
        }
        let events = self.tabs[i].term.drain_events();
        for event in events {
            self.tabs[i].shell_state.observe(&event);
        }
        // Cwd owned before the recorder borrow; drains collected before
        // shared App state (known) is touched.
        let cwd = shell_cwd(&self.tabs[i].session);
        let tab = &mut self.tabs[i];
        let (names, failed) = match &mut tab.recorder {
            Some(rec) => {
                rec.pump(&mut tab.term, &cwd);
                (rec.drain_cmd_names(), rec.drain_failed())
            }
            None => (Vec::new(), None),
        };
        // Shell-reported aliases/functions feed the router.
        self.known.insert_names(names);
        // Failures become did-you-mean offers (preview only).
        if let Some((cmd, cwd, output, exit)) = failed {
            let fix = zet_pty::suggest_fix(&cmd, &cwd, &output, exit, &self.known);
            if fix.is_some() {
                self.tabs[i].pending_fix = fix;
            }
        }
        // Alt switch swaps the visible grid: any selection addresses
        // stale rows, so drop it (both directions).
        let alt = self.tabs[i].term.alt_active();
        if alt != self.tabs[i].was_alt {
            self.tabs[i].selection = None;
            self.tabs[i].selecting = false;
            self.tabs[i].was_alt = alt;
        }
        died
    }

    /// Type one pending agent step visibly, then complete it from the
    /// recorded block. Single-lane: the agent waits, input is locked, so
    /// the newest block past `blocks_before` is our step's.
    fn service_exec(&mut self) {
        for i in 0..self.tabs.len() {
            self.service_exec_at(i);
        }
    }

    fn service_exec_at(&mut self, i: usize) {
        if self.tabs[i].exec_pending.is_none() {
            if let Some(rx) = &self.tabs[i].exec_req_rx {
                if let Ok(cmd) = rx.try_recv() {
                    let before = self.tabs[i]
                        .recorder
                        .as_ref()
                        .and_then(|r| r.blocks().ok())
                        .map(|b| b.len())
                        .unwrap_or(0);
                    self.tabs[i].exec_pending = Some(PendingExec {
                        cmd,
                        typed: false,
                        blocks_before: before,
                        started: std::time::Instant::now(),
                        watch: zet_gpu::exec_watch::StepWatch::new(
                            std::time::Duration::from_secs(120),
                            std::time::Duration::from_secs(10),
                        ),
                    });
                }
            }
        }
        let res: Option<ExecRes> = {
            let tab = &mut self.tabs[i];
            let Some(st) = &mut tab.exec_pending else {
                return;
            };
            if !st.typed {
                if let Some(s) = &mut tab.session {
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
            if tab.dead {
                Some((None, "shell exited".to_string()))
            } else if tab.recorder.is_none() {
                Some((None, "block recorder unavailable".to_string()))
            } else {
                let blocks = tab
                    .recorder
                    .as_ref()
                    .and_then(|r| r.blocks().ok())
                    .unwrap_or_default();
                let settled = blocks.len() > st.blocks_before;
                let elapsed = st.started.elapsed();
                match st.watch.poll(elapsed, settled) {
                    zet_gpu::exec_watch::StepPoll::Settle => blocks.last().map(|b| {
                        (
                            Some(b.exit_code),
                            format!("{}\n[exit {}]", b.output, b.exit_code),
                        )
                    }),
                    zet_gpu::exec_watch::StepPoll::Wait => None,
                    zet_gpu::exec_watch::StepPoll::Interrupt => {
                        // Stuck on stdin (pager, bat-on-stdin alias): ^C
                        // reclaims the prompt; the 130 block usually
                        // settles inside grace and completes normally.
                        if let Some(s) = &mut tab.session {
                            let _ = s.write_all(&[0x03]);
                        }
                        None
                    }
                    zet_gpu::exec_watch::StepPoll::Abandon => Some((
                        None,
                        "step produced no result after 120s; sent Ctrl+C and \
                         the prompt never returned"
                            .to_string(),
                    )),
                }
            }
        };
        if let Some(res) = res {
            if let Some(tx) = &self.tabs[i].exec_res_tx {
                let _ = tx.send(res);
            }
            self.tabs[i].exec_pending = None;
        }
    }

    /// Collect a finished F12 undo: sticky notice on the current line.
    /// A dead worker (panicked thread) clears the channel with a failure
    /// notice instead of locking F12 forever.
    fn poll_undo_done(&mut self) {
        for i in 0..self.tabs.len() {
            self.poll_undo_done_at(i);
        }
    }

    fn poll_undo_done_at(&mut self, i: usize) {
        let Some(rx) = &self.tabs[i].undo_rx else {
            return;
        };
        match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => (),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.tabs[i].undo_rx = None;
                let input = self.text_at(i).unwrap_or_default();
                self.tabs[i]
                    .ghost
                    .notify(input, "(undo failed: worker gone)".to_string());
            }
            Ok(res) => {
                self.tabs[i].undo_rx = None;
                let input = self.text_at(i).unwrap_or_default();
                match res {
                    Ok(()) => self.tabs[i].ghost.notify(input, "(undone)".to_string()),
                    Err(e) => self.tabs[i]
                        .ghost
                        .notify(input, format!("(undo failed: {e})")),
                }
            }
        }
    }

    /// Refresh the visible ghost suffix from current input. History only
    /// (sync, sub-ms); the model runs solely on NL Enter, in a thread.
    fn update_ghost(&mut self) {
        // NL answer landed: swap the NL line for the command (the preview).
        if let Some(rx) = &self.tabs[self.active].nl_rx {
            match rx.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.tabs[self.active].ghost.waiting();
                    return;
                }
                // Dead worker: drop back to history instead of "…" forever.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.tabs[self.active].nl_rx = None;
                    self.tabs[self.active].nl_wake = false;
                }
                // Model failures notice LOUD (rate limits, net, auth) —
                // never dissolve into `(no command)` again.
                Ok(Err(e)) => {
                    self.tabs[self.active].nl_rx = None;
                    self.tabs[self.active].nl_wake = false;
                    if let Some(input) = self.current_text() {
                        let short: String = e.chars().take(100).collect();
                        self.tabs[self.active]
                            .ghost
                            .notify(input, format!("(model error: {short})"));
                    }
                }
                Ok(Ok(answer)) => {
                    self.tabs[self.active].nl_rx = None;
                    if let Some(cmd) = answer {
                        // Conversation, not a command: answer it on the
                        // prompt line instead of swapping. `hello` gets a
                        // reply, not an exit 127.
                        if let Some(chat) = zet_suggest::chat_reply(&cmd) {
                            if let Some(input) = self.current_text() {
                                self.tabs[self.active]
                                    .ghost
                                    .notify(input, format!("agent: {chat}"));
                            }
                        // Never strand the shell in a continuation prompt:
                        // syntactically broken replies notice instead of swap.
                        } else if zet_pty::syntax_ok(&self.shell_name, &cmd) {
                            self.accept_line(&cmd);
                            // Wake-word orders (`zet update linux`) run:
                            // explicit instruction, T3 never auto-passes.
                            if self.tabs[self.active].nl_wake
                                && zet_policy::classify(&cmd) != zet_policy::Tier::T3
                            {
                                if let Some(s) = &mut self.tabs[self.active].session {
                                    let _ = s.write_all(b"\r");
                                }
                            } else if self.tabs[self.active].nl_wake {
                                self.tabs[self.active].ghost.notify(
                                    cmd.clone(),
                                    "(review: critical command — Enter to run)".to_string(),
                                );
                            }
                        } else if let Some(input) = self.current_text() {
                            let short: String = cmd.chars().take(60).collect();
                            self.tabs[self.active]
                                .ghost
                                .notify(input, format!("(invalid: {short})"));
                        }
                    } else if let Some(input) = self.current_text() {
                        self.tabs[self.active]
                            .ghost
                            .notify(input, "(no command)".to_string());
                    }
                    self.tabs[self.active].nl_wake = false;
                    self.retitle();
                }
            }
        }
        let input = self.prompt_input();
        self.tabs[self.active].ghost.update(input);
    }

    /// Recompute matches for the search query, jump to the first hit,
    /// and show the count on the prompt line.
    fn research(&mut self) {
        let active = self.active;
        let query = self.tabs[active].search.query.clone();
        let matches = self.tabs[active].term.search_matches(&query);
        let input = self.text_at(active).unwrap_or_default();
        let tab = &mut self.tabs[active];
        tab.search.matches = matches;
        tab.search.current = 0;
        if let Some(&(r, _, _)) = tab.search.matches.first() {
            tab.term.reveal_row(r);
        }
        let msg = if tab.search.query.is_empty() {
            "search: (type to find)".to_string()
        } else if tab.search.matches.is_empty() {
            format!("search: {} (no match)", tab.search.query)
        } else {
            format!(
                "search: {} (1/{})",
                tab.search.query,
                tab.search.matches.len()
            )
        };
        tab.ghost.notify(input, msg);
        self.needs_draw = true;
    }

    /// Step the current hit (dir = +1 next, -1 previous), wrapping.
    fn search_step(&mut self, dir: isize) {
        let active = self.active;
        let n = self.tabs[active].search.matches.len();
        if n == 0 {
            return;
        }
        let input = self.text_at(active).unwrap_or_default();
        let tab = &mut self.tabs[active];
        tab.search.current = (tab.search.current as isize + dir).rem_euclid(n as isize) as usize;
        let (r, _, _) = tab.search.matches[tab.search.current];
        tab.term.reveal_row(r);
        let msg = format!(
            "search: {} ({}/{})",
            tab.search.query,
            tab.search.current + 1,
            n
        );
        tab.ghost.notify(input, msg);
        self.needs_draw = true;
    }

    /// Leave search mode (view stays where it landed).
    fn search_exit(&mut self) {
        self.tabs[self.active].search.on = false;
        self.tabs[self.active].ghost.reset();
        self.needs_draw = true;
    }

    /// Offer a pending did-you-mean fix once the prompt is fresh.
    /// Called every frame: waits for the empty line, then previews.
    fn poll_fix_offer(&mut self) {
        if self.tabs[self.active].pending_fix.is_some()
            && self.current_text().is_some_and(|t| t.is_empty())
        {
            if let Some(fix) = self.tabs[self.active].pending_fix.take() {
                self.tabs[self.active].ghost.preview(fix);
            }
        }
    }

    fn current_text(&self) -> Option<String> {
        self.text_at(self.active)
    }

    /// Prompt-line text of a specific tab (poll fns use this: bg-tab
    /// completions must read their own line, not the visible one).
    fn text_at(&self, i: usize) -> Option<String> {
        self.tabs[i]
            .recorder
            .as_ref()
            .and_then(|rec| rec.current_input(&self.tabs[i].term))
            .map(|(text, _, _)| text)
    }

    /// Cell under the last known mouse position (absolute rows).
    fn mouse_cell(&self) -> Option<zet_gpu::selection::CellPos> {
        let (x, y) = self.mouse_px?;
        let r = self.renderer.as_ref()?;
        let (cw, ch) = r.cell_px();
        let vp = self.frame_layout().term;
        let gpu_vp = zet_gpu::renderer::Viewport {
            x: vp.x,
            y: vp.y,
            cols: vp.cols,
            rows: vp.rows,
        };
        zet_gpu::renderer::viewport_hit_test(
            x,
            y,
            gpu_vp,
            cw,
            ch,
            self.tabs[self.active].term.visible_base(),
        )
    }

    /// Copy the mouse selection to the clipboard (no-op when empty).
    fn copy_selection(&mut self) {
        let Some(sel) = &self.tabs[self.active].selection else {
            return;
        };
        let text = sel.text(&self.tabs[self.active].term);
        if text.is_empty() {
            return;
        }
        let input = self.current_text().unwrap_or_default();
        match arboard::Clipboard::new().and_then(|mut clip| clip.set_text(text)) {
            Ok(()) => {
                let (s, e) = sel.normalized();
                let lines = e.row - s.row + 1;
                let word = if lines == 1 { "line" } else { "lines" };
                self.tabs[self.active].ghost.notify(
                    input,
                    format!("(copied {lines} line{})", if lines == 1 { "" } else { "s" }),
                );
                // OS bubble too (best effort, off-thread so D-Bus never
                // hitches a frame; failures stay silent — the prompt
                // notice above is the reliable channel).
                std::thread::spawn(move || {
                    let _ = notify_rust::Notification::new()
                        .summary("Copied to clipboard")
                        .body(&format!("{lines} {word} from ZET"))
                        .show();
                });
            }
            Err(e) => {
                self.tabs[self.active]
                    .ghost
                    .notify(input, format!("(copy failed: {e})"));
            }
        }
    }

    /// Paste the system clipboard into the active terminal (Ctrl+Shift+V,
    /// Shift+Insert). Failures surface through the ghost notice like copy
    /// failures do; the notice is content-free — clipboard text is never
    /// printed, only that a failure occurred and why.
    fn paste_from_clipboard(&mut self) {
        let input = self.current_text().unwrap_or_default();
        let text = match arboard::Clipboard::new().and_then(|mut clip| clip.get_text()) {
            Ok(text) => text,
            Err(e) => {
                self.tabs[self.active]
                    .ghost
                    .notify(input, format!("(paste failed: {e})"));
                return;
            }
        };
        // Empty clipboard: nothing to paste, stay quiet.
        if text.is_empty() {
            return;
        }
        let bracketed = self.tabs[self.active].term.bracketed_paste();
        if let Some(s) = &mut self.tabs[self.active].session {
            let _ = s.write_all(&zet_gpu::input::paste_wrapped(&text, bracketed));
        }
    }

    /// Highlight the whole buffer (scrollback + grid) for copying.
    /// Follow with Ctrl+Shift+C or right-click to copy it.
    fn select_all(&mut self) {
        // Alt screen owns its grid: no scrollback above it.
        let total = if self.tabs[self.active].term.alt_active() {
            self.rows
        } else {
            self.tabs[self.active].term.scrollback_len() + self.rows
        };
        let sel = Selection::all(self.cols, total);
        self.tabs[self.active].selection = if sel.is_empty() { None } else { Some(sel) };
        self.tabs[self.active].selecting = false;
        self.needs_draw = true;
    }

    fn prompt_input(&self) -> Option<zet_suggest::PromptInput> {
        let (text, col, abs) = self.tabs[self.active]
            .recorder
            .as_ref()
            .and_then(|rec| rec.current_input(&self.tabs[self.active].term))?;
        let (cx, cy) = self.tabs[self.active].term.cursor();
        Some(zet_suggest::PromptInput {
            text,
            prompt_col: col,
            prompt_row: abs,
            cursor: (cx, self.tabs[self.active].term.scrollback_len() + cy),
        })
    }

    /// Replace the current line with `cmd` (Ctrl+U then type).
    fn accept_line(&mut self, cmd: &str) {
        self.tabs[self.active].ghost.reset();
        if let Some(s) = &mut self.tabs[self.active].session {
            let _ = s.write_all(&[0x15]);
            let _ = s.write_all(cmd.as_bytes());
        }
    }

    fn retitle(&self) {
        if let Some(w) = &self.window {
            let mode = if self.tabs[self.active].agent_busy {
                " (agent…)"
            } else if self.tabs[self.active].goal_mode {
                " (agent)"
            } else {
                ""
            };
            // Tab position only when it matters (more than one).
            let tabs = if self.tabs.len() > 1 {
                format!(" [{}/{}]", self.active + 1, self.tabs.len())
            } else {
                String::new()
            };
            w.set_title(&format!("ZET — {}{mode}{tabs}", self.shell_name));
        }
    }

    /// The active tab's shell has reported a prompt, so a goal can run
    /// in it.
    ///
    /// The shell reports the end of a command with OSC 133 `D;<code>`,
    /// which puts `ShellState` back at `Phase::Prompt` and records the
    /// exit code. That marker is also proof that the shell-integration
    /// hook is live — the agent's steps are recorded as blocks only
    /// from then on, so a goal started earlier would lose them.
    ///
    /// The phase alone cannot say this: `ShellState::new()` starts at
    /// `Phase::Prompt`, before the shell has said anything at all.
    fn shell_reported_prompt(&self) -> bool {
        let tab = &self.tabs[self.active];
        !tab.dead && tab.shell_state.last_exit().is_some()
    }

    /// Title variant carrying the last exit code (post-command).
    fn retitle_with_exit(&self, code: u32) {
        if let Some(w) = &self.window {
            let mode = if self.tabs[self.active].agent_busy {
                " (agent…)"
            } else if self.tabs[self.active].goal_mode {
                " (agent)"
            } else {
                ""
            };
            let tabs = if self.tabs.len() > 1 {
                format!(" [{}/{}]", self.active + 1, self.tabs.len())
            } else {
                String::new()
            };
            w.set_title(&format!(
                "ZET — {} (exit {code}){mode}{tabs}",
                self.shell_name
            ));
        }
    }

    /// Tab bar owns row 0 whenever more than one tab exists; the
    /// terminal grid shrinks by that row (all mappings offset).
    fn bar_rows(&self) -> usize {
        (self.tabs.len() > 1) as usize
    }

    /// Width of the currently resolved rail, in columns. Zero in the
    /// default Terminal Focus layout, so existing tab-bar behavior is
    /// unchanged until a rail is actually enabled.
    fn rail_cols(&self) -> usize {
        self.frame_layout().rail.map_or(0, |vp| vp.cols as usize)
    }

    #[allow(dead_code)]
    fn grid_rows(&self) -> usize {
        self.rows.saturating_sub(self.bar_rows())
    }
    fn frame_layout(&self) -> layout::FrameLayout {
        layout::resolve(
            self.cols as u32,
            self.rows as u32,
            self.layout,
            self.bar_rows(),
        )
    }

    fn refresh_chrome(&mut self) {
        self.refresh_chrome_at(std::time::Instant::now());
    }

    fn refresh_inspector(&mut self) {
        self.refresh_inspector_at(std::time::Instant::now());
    }

    pub fn refresh_inspector_at(&mut self, now: std::time::Instant) {
        if self.frame_layout().inspector.is_none() {
            if self.focus == Focus::Inspector {
                self.focus = Focus::Terminal;
                self.inspector_sel = None;
            }
            return;
        }
        if self.inspector_cache.last_active != self.active {
            self.inspector_sel = None;
        }
        let phase = self
            .tabs
            .get(self.active)
            .map(|tab| tab.shell_state.phase());
        let cwd = self
            .tabs
            .get(self.active)
            .map(|tab| shell_cwd(&tab.session))
            .unwrap_or_default();
        let tab_count = self.tabs.len();
        let active_dead = self
            .tabs
            .get(self.active)
            .map(|tab| tab.dead)
            .unwrap_or(true);
        let session_id = self
            .tabs
            .get(self.active)
            .map(|tab| tab.session_id)
            .unwrap_or(0);
        let cheap_changed = self.inspector_cache.cheap_changed(
            session_id,
            self.active,
            phase,
            &cwd,
            tab_count,
            active_dead,
        );
        if !cheap_changed && !self.inspector_cache.poll_due(now) {
            return;
        }
        #[cfg(test)]
        {
            // Keep test builds quiet; no counters are required for the
            // first Inspector slice.
        }
        let session_id = self
            .tabs
            .get(self.active)
            .map(|tab| tab.session_id)
            .unwrap_or(0);
        let snapshot = self.build_inspector_snapshot();
        self.inspector_cache.commit_refresh(
            session_id,
            self.active,
            phase,
            cwd,
            tab_count,
            active_dead,
            snapshot,
            now,
        );
    }

    fn build_inspector_snapshot(&self) -> panels::InspectorSnapshot {
        let tab = self.tabs.get(self.active);
        let raw_cwd = tab.map(|tab| shell_cwd(&tab.session)).unwrap_or_default();
        let cwd = panels::sanitize_inspector_text(&raw_cwd, 64);
        let state = tab
            .map(|tab| {
                if tab.dead {
                    "exited".to_string()
                } else {
                    match tab.shell_state.phase() {
                        zet_pty::shell::Phase::Prompt => "prompt".to_string(),
                        zet_pty::shell::Phase::Input => "input".to_string(),
                        zet_pty::shell::Phase::Running => "running".to_string(),
                    }
                }
            })
            .unwrap_or_else(|| "unknown".to_string());
        let exit = tab
            .and_then(|tab| tab.shell_state.last_exit())
            .map(|code| code.to_string())
            .unwrap_or_else(|| "—".to_string());
        let session = format!("{}", self.active + 1);
        let cwd = if cwd.is_empty() {
            "—".to_string()
        } else {
            cwd
        };
        panels::InspectorSnapshot {
            active: self.active,
            session,
            cwd,
            state,
            exit,
        }
    }

    pub fn refresh_chrome_at(&mut self, now: std::time::Instant) {
        if !self.layout.rail {
            return;
        }
        let phase = self
            .tabs
            .get(self.active)
            .map(|tab| tab.shell_state.phase());
        let cwd = self
            .tabs
            .get(self.active)
            .map(|tab| shell_cwd(&tab.session))
            .unwrap_or_default();
        let tab_count = self.tabs.len();
        let active_dead = self
            .tabs
            .get(self.active)
            .map(|tab| tab.dead)
            .unwrap_or(true);
        let cheap_changed = self.chrome.cheap_changed(
            self.active,
            self.layout.rail,
            phase,
            &cwd,
            tab_count,
            active_dead,
        );
        if !cheap_changed && !self.chrome.poll_due(now) {
            return;
        }
        #[cfg(test)]
        {
            self.chrome_query_count += 1;
        }
        let block_count = self
            .tabs
            .get(self.active)
            .and_then(|tab| tab.recorder.as_ref())
            .map(|recorder| recorder.block_count())
            .unwrap_or(0);
        if !cheap_changed && block_count == self.chrome.last_blocks {
            self.chrome.mark_refreshed(now);
            return;
        }
        #[cfg(test)]
        {
            self.chrome_build_count += 1;
        }
        let snapshot = self.build_rail_snapshot();
        self.chrome.commit_refresh(
            self.active,
            self.layout.rail,
            block_count,
            phase,
            cwd,
            tab_count,
            active_dead,
            snapshot,
            now,
        );
    }

    fn build_rail_snapshot(&self) -> panels::RailSnapshot {
        let mode = if self.layout.rail {
            "Workspace"
        } else {
            "Terminal Focus"
        };
        let sessions = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let cwd = shell_cwd(&tab.session);
                let cwd_tail = if cwd.is_empty() {
                    "—".to_string()
                } else {
                    cwd.rsplit('/').next().unwrap_or(&cwd).to_string()
                };
                let state = if tab.dead {
                    "exited".to_string()
                } else {
                    match tab.shell_state.phase() {
                        zet_pty::shell::Phase::Prompt => "prompt".to_string(),
                        zet_pty::shell::Phase::Input => "input".to_string(),
                        zet_pty::shell::Phase::Running => "running".to_string(),
                    }
                };
                let exit = tab
                    .shell_state
                    .last_exit()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "—".to_string());
                panels::SessionRow {
                    index: i + 1,
                    cwd_tail,
                    shell: self.shell_name.clone(),
                    state,
                    exit,
                    provenance: panels::Provenance::Inferred,
                }
            })
            .collect();
        let commands = self
            .tabs
            .get(self.active)
            .and_then(|tab| tab.recorder.as_ref())
            .map(|recorder| {
                recorder
                    .recent_commands(8)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|block| panels::CommandRow {
                        command: block.command,
                        exit: block.exit_code.to_string(),
                        duration: if block.duration_ms == 0 {
                            "—".to_string()
                        } else {
                            format!("{}ms", block.duration_ms)
                        },
                        provenance: panels::Provenance::Observed,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let footer = self
            .tabs
            .get(self.active)
            .map(|tab| shell_cwd(&tab.session))
            .unwrap_or_default();
        let (footer, footer_provenance) = if footer.is_empty() {
            ("—".to_string(), panels::Provenance::Unknown)
        } else {
            (footer, panels::Provenance::Observed)
        };
        panels::RailSnapshot {
            mode: mode.to_string(),
            active: self.active,
            sessions,
            commands,
            footer,
            footer_provenance,
        }
    }

    /// Size every tab's grid + PTY to the terminal viewport.
    fn fit_all(&mut self) {
        let (cols, rows) = layout::terminal_viewport(
            self.layout,
            self.cols as u32,
            self.rows as u32,
            self.bar_rows(),
        );
        let (cols, rows) = (cols as usize, rows as usize);
        for tab in &mut self.tabs {
            tab.term.resize(cols, rows);
            if let Some(s) = &tab.session {
                let _ = s.resize(cols.min(32767) as u16, rows.min(32767) as u16);
            }
        }
    }

    fn toggle_rail(&mut self) {
        self.layout.rail = !self.layout.rail;
        self.focus = Focus::Terminal;
        self.chrome.mark_dirty();
        self.fit_all();
        self.needs_draw = true;
        self.retitle();
    }

    fn toggle_inspector(&mut self) {
        self.layout.inspector = !self.layout.inspector;
        if !self.layout.inspector {
            self.focus = Focus::Terminal;
            self.inspector_sel = None;
        }
        self.inspector_cache.mark_dirty();
        self.fit_all();
        self.needs_draw = true;
    }

    fn handle_mouse_click(&mut self, x: f64, y: f64, cw: u32, ch: u32) {
        self.needs_draw = true;
        // Tab bar owns pixel row 0: clicks switch tabs.
        if self.bar_rows() > 0
            && x >= 0.0
            && y >= 0.0
            && (y / ch.max(1) as f64).floor() as usize == 0
        {
            let col = (x / cw.max(1) as f64).floor() as usize;
            if col >= self.cols {
                return;
            }
            if let Some(seg) = zet_gpu::selection::tabbar_hit_segment(
                col,
                self.rail_cols(),
                self.cols,
                self.tabs.len(),
            ) {
                self.active = seg;
                self.inspector_cache.mark_dirty();
                self.inspector_sel = None;
                self.focus = Focus::Terminal;
                self.needs_draw = true;
                self.retitle();
            }
            return;
        }
        // Rail clicks focus the rail instead of selecting terminal cells.
        let rail_cols = self.rail_cols();
        if rail_cols > 0 {
            let vp = self.frame_layout().term;
            let rail_px = rail_cols as f64 * cw as f64;
            let top = vp.y as f64 * ch as f64;
            let bottom = (vp.y as f64 + vp.rows as f64) * ch as f64;
            if x >= 0.0 && x < rail_px && y >= top && y < bottom {
                let row = ((y - top) / ch as f64).floor() as usize as u32;
                if let Some(session) = panels::rail_row_target(row, self.tabs.len(), vp.rows) {
                    self.rail_sel = session;
                }
                self.focus = Focus::Rail;
                self.needs_draw = true;
                return;
            }
        }
        // Inspector clicks focus the Inspector instead of selecting
        // terminal cells.
        if let Some(insp_vp) = self.frame_layout().inspector {
            let left = insp_vp.x as f64 * cw as f64;
            let right = (insp_vp.x + insp_vp.cols) as f64 * cw as f64;
            let top = insp_vp.y as f64 * ch as f64;
            let bottom = (insp_vp.y + insp_vp.rows) as f64 * ch as f64;
            if x >= left && x < right && y >= top && y < bottom {
                self.focus = Focus::Inspector;
                let row = ((y - top) / ch as f64).floor() as usize;
                self.inspector_sel = if (1..=4).contains(&row) && (row as u32) < insp_vp.rows {
                    Some(row - 1)
                } else {
                    None
                };
                self.needs_draw = true;
                return;
            }
        }
        let vp = self.frame_layout().term;
        let gpu_vp = zet_gpu::renderer::Viewport {
            x: vp.x,
            y: vp.y,
            cols: vp.cols,
            rows: vp.rows,
        };
        if let Some(cell) = zet_gpu::renderer::viewport_hit_test(
            x,
            y,
            gpu_vp,
            cw,
            ch,
            self.tabs[self.active].term.visible_base(),
        ) {
            self.focus = Focus::Terminal;
            self.tabs[self.active].selecting = true;
            self.tabs[self.active].selection =
                Some(Selection::new((cell.col, cell.row), (cell.col, cell.row)));
        }
    }

    fn draw_rail(
        r: &mut Renderer,
        vp: zet_gpu::renderer::Viewport,
        snapshot: &panels::RailSnapshot,
        selected: usize,
    ) {
        use zet_gpu::quad::Rgba;
        let ink = zet_gpu::renderer::TAB_DIM_INK;
        let rows = panels::rail_rows(vp.rows, snapshot.sessions.len(), snapshot.commands.len());
        for (row, kind) in rows.into_iter().enumerate() {
            let row = row as u32;
            match kind {
                panels::RailRow::Header => {
                    r.draw_text_row(vp, row, 0, "ZET", Rgba::WHITE);
                }
                panels::RailRow::Mode => {
                    r.draw_text_row(vp, row, 0, &format!("mode {}", snapshot.mode), ink);
                }
                panels::RailRow::SessionsHeader => {
                    r.draw_text_row(vp, row, 0, "SESSIONS", ink);
                }
                panels::RailRow::Session(index) => {
                    if index == selected && index < snapshot.sessions.len() {
                        r.draw_fill(
                            zet_gpu::renderer::Viewport {
                                x: 0,
                                y: vp.y + row,
                                cols: vp.cols,
                                rows: 1,
                            },
                            zet_gpu::renderer::TAB_ACTIVE_BG,
                        );
                    }
                    let session = &snapshot.sessions[index];
                    let marker = if index == snapshot.active { "▸" } else { " " };
                    let line = panels::format_session_row(
                        marker,
                        session.index,
                        &session.state,
                        &session.cwd_tail,
                        vp.cols,
                    );
                    let line_ink = if index == snapshot.active {
                        Rgba::WHITE
                    } else {
                        ink
                    };
                    r.draw_text_row(vp, row, 0, &line, line_ink);
                }
                panels::RailRow::CommandsHeader => {
                    r.draw_text_row(vp, row, 0, "OBSERVED COMMANDS", ink);
                }
                panels::RailRow::Command(index) => {
                    if let Some(command) = snapshot.commands.get(index) {
                        let line = panels::format_command_row(
                            &command.command,
                            &command.exit,
                            &command.duration,
                            vp.cols,
                        );
                        r.draw_text_row(vp, row, 0, &line, ink);
                    }
                }
                panels::RailRow::Footer => {
                    let footer = panels::format_footer(&snapshot.footer, vp.cols);
                    r.draw_text_row(vp, row, 0, &footer, ink);
                }
                panels::RailRow::Empty => {}
            }
        }
    }

    /// Short per-tab labels: index + cwd tail, truncated to segment.
    fn tab_labels(&self) -> Vec<String> {
        let n = self.tabs.len().max(1);
        let seg_w = (self.cols / n).max(4);
        self.tabs
            .iter()
            .enumerate()
            .map(|(k, tab)| {
                let cwd = shell_cwd(&tab.session);
                let tail = cwd.rsplit('/').next().unwrap_or(&cwd);
                let name: String = tail
                    .chars()
                    .rev()
                    .take(seg_w - 4)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                format!(" {} {name}", k + 1)
            })
            .collect()
    }

    /// Open a fresh shell tab and switch to it.
    fn new_tab(&mut self) {
        let Some(wake) = self.wake.clone() else {
            return;
        };
        self.tabs
            .push(Tab::spawn(&self.prog, self.hook.clone(), wake));
        self.active = self.tabs.len() - 1;
        self.inspector_cache.mark_dirty();
        self.inspector_sel = None;
        if let Some(renderer) = &self.renderer {
            let (fg, bg) = renderer.default_colors();
            if let Some(tab) = self.tabs.last_mut() {
                tab.term.set_default_colors(fg, bg);
            }
        }
        // New grid + bar toggle may shrink every tab: refit all.
        self.fit_all();
        self.needs_draw = true;
        self.retitle();
    }

    /// Close the active tab: the session is shut down (kill + wait,
    /// no zombies) before the tab drops. The last tab closes the
    /// window instead of stranding it.
    fn close_tab(&mut self, event_loop: &ActiveEventLoop) {
        if self.tabs.len() <= 1 {
            event_loop.exit();
            return;
        }
        let mut tab = self.tabs.remove(self.active);
        if let Some(s) = tab.session.as_mut() {
            s.shutdown();
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.inspector_cache.mark_dirty();
        self.inspector_sel = None;
        // Bar may vanish:: remaining grids grow back.
        self.fit_all();
        self.needs_draw = true;
        self.retitle();
    }

    /// Cycle tabs (dir = +1 next, -1 previous). No-op solo.
    fn cycle_tab(&mut self, dir: isize) {
        if self.tabs.len() <= 1 {
            return;
        }
        let n = self.tabs.len() as isize;
        self.active = (self.active as isize + dir).rem_euclid(n) as usize;
        self.inspector_cache.mark_dirty();
        self.inspector_sel = None;
        self.needs_draw = true;
        self.retitle();
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
                self.tabs[self.active].ghost.notify(
                    nl,
                    "(no model: `zet inline <spec>` or ZET_INLINE_SPEC)".to_string(),
                );
                return false;
            }
        };
        if let Err(e) = Self::check_spec(&spec) {
            self.tabs[self.active].ghost.notify(nl, format!("({e})"));
            return false;
        }
        let (tx, rx) = mpsc::channel();
        self.tabs[self.active].nl_rx = Some(rx);
        self.tabs[self.active].nl_wake = wake;
        // M1 for NL too: the request sees recent blocks + cwd + repo
        // state, so "fix this" needs no explaining. Gathered here (main
        // thread, needs the recorder), moved into the worker.
        let cwd = shell_cwd(&self.tabs[self.active].session);
        let context = self.tabs[self.active]
            .recorder
            .as_ref()
            .and_then(|r| r.blocks().ok())
            .map(|blocks| {
                let git = zet_context::git_state(std::path::Path::new(&cwd));
                zet_context::gather(&blocks, &cwd, git, zet_context::DEFAULT_BUDGET).render()
            });
        std::thread::spawn(move || {
            // check_spec passed, so the client builds; a race (key
            // deleted between) reports instead of going silent.
            let answer: Result<Option<String>, String> = match build_suggester(&spec) {
                None => Err("model unavailable".to_string()),
                Some(m) => m
                    .suggest_nl(&nl, context.as_deref())
                    .map_err(|e| e.to_string()),
            };
            let _ = tx.send(answer);
        });
        true
    }

    /// Start an agent turn on a thread: each approved step is typed
    /// visibly into the PTY and completed from its recorded block.
    /// Entering agent mode pre-consents T0-T2; T3 is never approved.
    /// `plan` is the approved step list (planner loop, phase two).
    fn start_agent(&mut self, goal: String, plan: Option<Vec<String>>) {
        let spec = match self.resolve_spec() {
            Some(s) => s,
            None => {
                self.tabs[self.active].goal_mode = false;
                self.retitle();
                self.tabs[self.active].ghost.notify(
                    goal,
                    "(no model: `zet inline <spec>` or ZET_INLINE_SPEC)".to_string(),
                );
                return;
            }
        };
        if let Err(e) = Self::check_spec(&spec) {
            self.tabs[self.active].goal_mode = false;
            self.retitle();
            self.tabs[self.active].ghost.notify(goal, format!("({e})"));
            return;
        }
        let cwd = shell_cwd(&self.tabs[self.active].session);
        let in_repo = zet_agent::detect_repo(std::path::Path::new(&cwd));
        // M1: the turn sees what the user sees — recent blocks (scrubbed,
        // budgeted), cwd, repo state. Best effort: failures degrade to
        // no-context rather than blocking the turn.
        let context = self.tabs[self.active]
            .recorder
            .as_ref()
            .and_then(|r| r.blocks().ok())
            .map(|blocks| {
                let git = zet_context::git_state(std::path::Path::new(&cwd));
                zet_context::gather(&blocks, &cwd, git, zet_context::DEFAULT_BUDGET).render()
            });
        let (req_tx, req_rx) = mpsc::channel::<String>();
        let (res_tx, res_rx) = mpsc::channel::<ExecRes>();
        let (done_tx, done_rx) = mpsc::channel::<Option<AgentDone>>();
        self.tabs[self.active].exec_req_rx = Some(req_rx);
        self.tabs[self.active].exec_res_tx = Some(res_tx);
        self.tabs[self.active].agent_rx = Some(done_rx);
        self.tabs[self.active].goal_mode = false;
        self.tabs[self.active].agent_busy = true;
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
                if let Some(context) = context {
                    agent = agent.with_context(context);
                }
                if let Some(plan) = plan {
                    agent = agent.with_plan(plan);
                }
                let summary = agent
                    .run(&goal)
                    .unwrap_or_else(|e| format!("agent error: {e}"));
                (summary, agent.journal)
            });
            let _ = done_tx.send(done);
        });
    }

    /// Planner loop, phase one: draft the step list on a thread. Trivial
    /// goals (0-1 steps) run immediately; real plans wait for one Enter.
    fn start_plan(&mut self, goal: String) {
        let spec = match self.resolve_spec() {
            Some(s) => s,
            None => {
                self.tabs[self.active].goal_mode = false;
                self.retitle();
                self.tabs[self.active].ghost.notify(
                    goal,
                    "(no model: `zet inline <spec>` or ZET_INLINE_SPEC)".to_string(),
                );
                return;
            }
        };
        if let Err(e) = Self::check_spec(&spec) {
            self.tabs[self.active].goal_mode = false;
            self.retitle();
            self.tabs[self.active].ghost.notify(goal, format!("({e})"));
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.tabs[self.active].plan_rx = Some(rx);
        self.retitle();
        std::thread::spawn(move || {
            // Fail closed: a broken model must never skip the gate into
            // an instant ungated run. Errors surface as notices.
            let steps = match build_client(&spec) {
                None => Err("model unavailable".to_string()),
                Some(client) => zet_agent::draft_plan(&client, &goal).map_err(|e| e.to_string()),
            };
            let _ = tx.send((goal, steps));
        });
    }

    /// Collect a drafted plan: trivial runs now, the rest waits for one
    /// Enter (Esc drops it). Single lane with agent turns: a running
    /// turn blocks planning like everything else.
    fn poll_plan(&mut self) {
        for i in 0..self.tabs.len() {
            self.poll_plan_at(i);
        }
    }

    fn poll_plan_at(&mut self, i: usize) {
        let Some(rx) = &self.tabs[i].plan_rx else {
            return;
        };
        let (goal, ready) = match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.tabs[i].plan_rx = None;
                return;
            }
            Ok(ready) => ready,
        };
        self.tabs[i].plan_rx = None;
        match ready {
            Err(e) => {
                self.tabs[i]
                    .ghost
                    .notify(goal, format!("(plan failed: {e})"));
            }
            Ok(steps) => {
                if steps.len() <= 1 {
                    // Trivial: no gate worth holding. The Enter-time goal
                    // runs (not whatever was typed during the draft).
                    self.start_agent(goal, None);
                } else {
                    let mut text = String::from("plan:");
                    for (i, step) in steps.iter().enumerate() {
                        text.push_str(&format!("\n{}. {}", i + 1, step));
                    }
                    text.push_str("\n(Enter: run it, Esc: drop it)");
                    self.tabs[i].ghost.notify(goal.clone(), text);
                    self.tabs[i].pending_plan = Some((goal, steps));
                }
            }
        }
    }

    /// Collect a finished agent turn: journal kept, summary as a sticky
    /// notice on the fresh prompt, modes cleared. A dead worker unlocks
    /// input with a failure notice instead of holding the lock forever.
    fn poll_agent_done(&mut self) {
        for i in 0..self.tabs.len() {
            self.poll_agent_done_at(i);
        }
    }

    fn poll_agent_done_at(&mut self, i: usize) {
        let Some(rx) = &self.tabs[i].agent_rx else {
            return;
        };
        match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => (),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.tabs[i].agent_rx = None;
                self.tabs[i].agent_busy = false;
                self.tabs[i].exec_req_rx = None;
                self.tabs[i].exec_res_tx = None;
                self.tabs[i].exec_pending = None;
                self.retitle();
                let input = self.text_at(i).unwrap_or_default();
                self.tabs[i]
                    .ghost
                    .notify(input, "(agent failed: worker gone)".to_string());
            }
            Ok(done) => {
                self.tabs[i].agent_rx = None;
                self.tabs[i].agent_busy = false;
                self.tabs[i].exec_req_rx = None;
                self.tabs[i].exec_res_tx = None;
                self.tabs[i].exec_pending = None;
                self.retitle();
                let (input, text) = match done {
                    Some((summary, journal)) => {
                        // Best effort: the turn survives the window via `zet journal`.
                        let _ =
                            zet_agent::append_journal(&zet_agent::journal_path_default(), &journal);
                        self.tabs[i].last_journal = journal;
                        let first: String = summary
                            .lines()
                            .next()
                            .unwrap_or("done")
                            .chars()
                            .take(120)
                            .collect();
                        (
                            self.text_at(i).unwrap_or_default(),
                            format!("agent: {first}"),
                        )
                    }
                    None => (
                        self.text_at(i).unwrap_or_default(),
                        "(agent: no model — check spec/key)".to_string(),
                    ),
                };
                self.tabs[i].ghost.notify(input, text);
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

    pub fn handle_key_parts(
        &mut self,
        logical: winit::keyboard::Key,
        text: Option<&str>,
        pressed: bool,
        mods: winit::keyboard::ModifiersState,
        event_loop: Option<&ActiveEventLoop>,
    ) {
        // Any key gets a prompt frame; the heartbeat covers the rest.
        self.needs_draw = true;
        // Typing returns to live (the cursor is there, not in history).
        // Search mode keeps its revealed view (snap would fight it).
        if pressed && self.focus == Focus::Terminal && !self.tabs[self.active].search.on {
            self.tabs[self.active].term.snap_to_bottom();
        }
        if pressed && self.perf_t0.is_some() {
            self.perf_input = Some(std::time::Instant::now());
        }
        // Panel focus owns the keyboard: typing never leaks to
        // the PTY while the rail is focused.
        if self.focus != Focus::Terminal {
            if !pressed {
                return;
            }
            if is_rail_toggle(&logical, pressed, &mods) {
                self.toggle_rail();
                return;
            }
            if is_inspector_toggle(&logical, pressed, &mods) {
                self.toggle_inspector();
                return;
            }
            match &logical {
                Key::Named(NamedKey::Escape) => {
                    self.focus = Focus::Terminal;
                    self.inspector_sel = None;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    if self.focus == Focus::Rail && !self.tabs.is_empty() {
                        self.rail_sel = self.rail_sel.saturating_sub(1);
                    }
                    if self.focus == Focus::Inspector {
                        self.inspector_sel = match self.inspector_sel {
                            Some(0) | None => Some(3),
                            Some(n) => Some(n - 1),
                        };
                    }
                }
                Key::Named(NamedKey::ArrowDown) => {
                    if self.focus == Focus::Rail && !self.tabs.is_empty() {
                        self.rail_sel = (self.rail_sel + 1).min(self.tabs.len() - 1);
                    }
                    if self.focus == Focus::Inspector {
                        self.inspector_sel = match self.inspector_sel {
                            Some(3) => Some(0),
                            Some(n) => Some(n + 1),
                            None => Some(0),
                        };
                    }
                }
                Key::Named(NamedKey::Enter)
                    if self.focus == Focus::Rail && self.rail_sel < self.tabs.len() =>
                {
                    self.active = self.rail_sel;
                    self.inspector_cache.mark_dirty();
                    self.inspector_sel = None;
                    self.focus = Focus::Terminal;
                    self.retitle();
                }
                _ => {}
            }
            let ctrl_c = mods.contains(ModifiersState::CONTROL)
                && matches!(&logical, Key::Character(s) if s.eq_ignore_ascii_case("c"));
            if ctrl_c {
                // A safe route back to the terminal; the byte is
                // then translated and sent normally below, preserving
                // the emergency-cancellation behavior.
                self.focus = Focus::Terminal;
                self.inspector_sel = None;
            } else {
                self.needs_draw = true;
                return;
            }
        }
        // Agent running: input locked (its steps own the line).
        // Copy is local, not shell input: reading the grid never
        // disturbs the turn, so it stays available while busy.
        // Ctrl+C still reaches the shell: the emergency brake that
        // cancels the agent's running command (D fires, step ends).
        if self.tabs[self.active].agent_busy {
            if is_copy(&logical, pressed, &mods) {
                self.copy_selection();
                return;
            }
            // Select-all is local grid reading: safe while busy.
            if is_select_all(&logical, pressed, &mods) {
                self.select_all();
                return;
            }
            if let Some(bytes) = translate(&logical, text, pressed, &mods) {
                if bytes == [0x03] {
                    if let Some(s) = &mut self.tabs[self.active].session {
                        let _ = s.write_all(&bytes);
                    }
                }
            }
            return;
        }
        // Paste first: Ctrl+Shift+V would otherwise type a SYN byte.
        if is_paste(&logical, pressed, &mods) {
            self.paste_from_clipboard();
            return;
        }
        // Copy: Ctrl+Shift+C copies the mouse selection (never ETX).
        if is_copy(&logical, pressed, &mods) {
            self.copy_selection();
            return;
        }
        // Select all: Ctrl+Shift+A highlights scrollback + grid.
        // (Bare Ctrl+A stays readline's line-start.)
        if is_select_all(&logical, pressed, &mods) {
            self.select_all();
            return;
        }
        // Tabs: new/close/cycle never reach the shell.
        if is_new_tab(&logical, pressed, &mods) {
            self.new_tab();
            return;
        }
        if is_close_tab(&logical, pressed, &mods) {
            if let Some(event_loop) = event_loop {
                self.close_tab(event_loop);
            }
            return;
        }
        if let Some(dir) = is_cycle_tab(&logical, pressed, &mods) {
            self.cycle_tab(dir);
            return;
        }
        // Mission Rail toggle (Ctrl+Shift+M).
        if is_rail_toggle(&logical, pressed, &mods) {
            self.toggle_rail();
            return;
        }
        // Contextual Inspector toggle (Ctrl+Shift+I).
        if is_inspector_toggle(&logical, pressed, &mods) {
            self.toggle_inspector();
            return;
        }
        // Scrollback find: typing goes to the query, never the PTY.
        if is_search(&logical, pressed, &mods) && !self.tabs[self.active].search.on {
            self.tabs[self.active].search.on = true;
            self.tabs[self.active].search.query.clear();
            self.research();
            return;
        }
        if self.tabs[self.active].search.on {
            if !pressed {
                return;
            }
            let shift = mods.contains(ModifiersState::SHIFT);
            match &logical {
                Key::Named(NamedKey::Escape) => self.search_exit(),
                Key::Named(NamedKey::Enter) => self.search_step(if shift { -1 } else { 1 }),
                Key::Named(NamedKey::Backspace) => {
                    self.tabs[self.active].search.query.pop();
                    self.research();
                }
                Key::Character(_) => {
                    let ctrl = mods.contains(ModifiersState::CONTROL);
                    if !ctrl {
                        if let Some(text) = text {
                            self.tabs[self.active].search.query.push_str(text);
                            self.research();
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        // Agent goal mode toggle: Ctrl+G would otherwise type BEL.
        if is_agent_toggle(&logical, pressed, &mods) {
            self.tabs[self.active].goal_mode = !self.tabs[self.active].goal_mode;
            self.tabs[self.active].ghost.reset();
            self.tabs[self.active].nl_rx = None;
            self.tabs[self.active].nl_wake = false;
            // Leaving goal mode abandons any draft, or it could
            // land and launch after the mode is gone.
            self.tabs[self.active].plan_rx = None;
            self.tabs[self.active].pending_plan = None;
            self.retitle();
            return;
        }
        let is_tab = pressed && matches!(&logical, Key::Named(NamedKey::Tab));
        let is_enter = pressed && matches!(&logical, Key::Named(NamedKey::Enter));
        let is_esc = pressed && matches!(&logical, Key::Named(NamedKey::Escape));
        // F12: one-key undo of the last checkpointed agent step.
        // (While busy the early gate above already swallowed this.)
        // Runs on a thread: git in a giant repo must not hitch frames.
        if pressed && matches!(&logical, Key::Named(NamedKey::F12)) {
            let input = self.current_text().unwrap_or_default();
            if self.tabs[self.active].undo_rx.is_some() {
                // Never swallow silently: a stuck undo used to eat
                // every later F12 with zero feedback.
                self.tabs[self.active]
                    .ghost
                    .notify(input, "(undo already running)".to_string());
                return;
            }
            let cp = self.tabs[self.active]
                .last_journal
                .iter()
                .rev()
                .find_map(|e| e.checkpoint.clone());
            match cp {
                None => {
                    self.tabs[self.active]
                        .ghost
                        .notify(input, "(nothing to undo)".to_string());
                }
                Some(cp) => {
                    let cwd = shell_cwd(&self.tabs[self.active].session);
                    let (tx, rx) = mpsc::channel();
                    self.tabs[self.active].undo_rx = Some(rx);
                    std::thread::spawn(move || {
                        // Wrong-repo use fails closed: reset --hard
                        // with an unknown SHA errors, touching nothing.
                        let res = zet_agent::undo_checkpoint(std::path::Path::new(&cwd), &Some(cp))
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
            if let Some(suffix) = self.tabs[self.active].ghost.accept() {
                if let Some(s) = &mut self.tabs[self.active].session {
                    let _ = s.write_all(suffix.as_bytes());
                }
                return;
            }
        }
        // Escape clears ghost / cancels an in-flight answer / exits
        // goal mode; swallowed only when it did something.
        if is_esc {
            let was_pending = self.tabs[self.active].nl_rx.is_some();
            let was_goal = self.tabs[self.active].goal_mode;
            let had_fix = self.tabs[self.active].pending_fix.take().is_some();
            let swallowed = self.tabs[self.active].ghost.cancel();
            if was_pending || was_goal {
                self.tabs[self.active].ghost.dismiss_current();
            }
            self.tabs[self.active].nl_rx = None;
            self.tabs[self.active].nl_wake = false;
            self.tabs[self.active].plan_rx = None;
            self.tabs[self.active].pending_plan = None;
            if self.tabs[self.active].goal_mode {
                self.tabs[self.active].goal_mode = false;
                self.retitle();
            }
            if swallowed || was_pending || was_goal || had_fix {
                return;
            }
        }
        // Enter in goal mode starts an agent turn on the line.
        // With a pending plan, this Enter is the once-and-done
        // approval: the agreed steps run under normal policy.
        // The stored goal runs (approval was for it, not for
        // whatever was typed since).
        if is_enter && self.tabs[self.active].goal_mode {
            let input = self.current_text().unwrap_or_default();
            if !input.trim().is_empty() {
                if let Some((goal, plan)) = self.tabs[self.active].pending_plan.take() {
                    self.start_agent(goal, Some(plan));
                } else {
                    self.start_plan(input);
                }
            }
            return;
        }
        // Modeless routing: `zet <words>` translates the rest;
        // known commands run; unknown first words translate
        // (preview swaps in, never runs blind). The line stays
        // put when there's no model (notice shown).
        if is_enter && self.tabs[self.active].nl_rx.is_none() {
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
        if is_enter && self.tabs[self.active].nl_rx.is_some() {
            return;
        }
        if let Some(mut bytes) = translate(&logical, text, pressed, &mods) {
            // Typing clears the mouse selection.
            if pressed {
                self.tabs[self.active].selection = None;
            }
            // Send exactly what the line discipline erases with.
            if bytes == [0x7f] {
                bytes = vec![self.tabs[self.active].erase];
            }
            if let Some(s) = &mut self.tabs[self.active].session {
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
fn build_suggester(
    spec: &str,
) -> Option<zet_suggest::ModelSuggester<zet_models::Metered<zet_models::Client>>> {
    build_client(spec).map(zet_suggest::ModelSuggester::new)
}

/// Concrete model client for agent turns (Send; built per request).
/// The opencode: provider yields the Go gateway client (session
/// headers + chat/responses fallback); everything else is OpenAI-shape.
/// Metered: every call logs estimated tokens to ~/.zet/usage.jsonl.
fn build_client(spec: &str) -> Option<zet_models::Metered<zet_models::Client>> {
    use zet_models::Provider;
    let provider = with_keychains(|chains| zet_models::keys::resolve_provider(spec, chains).ok())?;
    let client = match provider {
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
    };
    Some(zet_models::Metered::new(
        client,
        spec,
        zet_models::metered::default_log(),
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
        self.cols = cols;
        self.rows = rows;
        // fit_all shrinks grids by the tab-bar row when tabbed.
        self.fit_all();
        self.window = Some(window);
        self.renderer = Some(renderer);
        if let Some(renderer) = &self.renderer {
            let (fg, bg) = renderer.default_colors();
            for tab in &mut self.tabs {
                tab.term.set_default_colors(fg, bg);
            }
        }
        // Heartbeat re-armed every about_to_wait; this initial value only
        // covers the first idle gap (events and the PTY proxy wake sooner).
        event_loop.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + FRAME));
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // PTY reader thread has bytes: prompt frame, coalesced by the flag.
        self.needs_draw = true;
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // A goal handed to us on the command line (`zet "fix the test"`)
        // starts as soon as the shell reports its first prompt.
        if self.pending_goal.is_some() && self.shell_reported_prompt() {
            if let Some(goal) = self.pending_goal.take() {
                self.start_agent(goal, None);
            }
        }
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
                self.handle_key_parts(
                    event.logical_key.clone(),
                    event.text.as_deref(),
                    event.state == ElementState::Pressed,
                    self.mods,
                    Some(event_loop),
                );
            }

            WindowEvent::Resized(size) => {
                self.needs_draw = true;
                if let Some(r) = &mut self.renderer {
                    let (cw, ch) = r.cell_px();
                    r.resize(size.width, size.height);
                    let cols = ((size.width / cw).max(20) as usize).min(500);
                    let rows = ((size.height / ch).max(10) as usize).min(200);
                    self.cols = cols;
                    self.rows = rows;
                    // fit_all shrinks grids by the tab-bar row when tabbed.
                    self.fit_all();
                }
            }
            WindowEvent::RedrawRequested => {
                self.pump();
                if self.tabs[self.active].dead {
                    event_loop.exit();
                    return;
                }
                // Drag-select autoscroll: holding the mouse past the
                // top/bottom edge walks the view each frame so the
                // selection reaches off-screen text. needs_draw only
                // when something moved, or the loop would spin.
                if self.tabs[self.active].selecting {
                    if let (Some((_, my)), Some(r)) = (self.mouse_px, self.renderer.as_ref()) {
                        let (_, ch) = r.cell_px();
                        let view_h = self.rows as f64 * ch.max(1) as f64;
                        let lines =
                            zet_gpu::selection::drag_scroll_lines(my, view_h, ch.max(1) as f64);
                        if lines != 0 {
                            let before = self.tabs[self.active].term.view_offset();
                            if lines > 0 {
                                self.tabs[self.active].term.scroll_view_up(lines as usize);
                            } else {
                                self.tabs[self.active]
                                    .term
                                    .scroll_view_down((-lines) as usize);
                            }
                            if let Some(cell) = self.mouse_cell() {
                                if let Some(sel) = &mut self.tabs[self.active].selection {
                                    let next = zet_gpu::selection::CellPos {
                                        col: cell.col,
                                        row: cell.row,
                                    };
                                    if sel.active != next {
                                        sel.active = next;
                                        self.needs_draw = true;
                                    }
                                }
                            }
                            if self.tabs[self.active].term.view_offset() != before {
                                self.needs_draw = true;
                            }
                        }
                    }
                }
                // Snapshot draw params before borrowing the renderer:
                // bar offset, labels, selection are Copy/owned.
                let (bar, cols, active) = (self.bar_rows(), self.cols, self.active);
                let labels = self.tab_labels();
                let sel = self.tabs[self.active].selection;
                let ghost = self.tabs[self.active].ghost.visible().map(|g| {
                    let (cx, cy) = self.tabs[self.active].term.cursor();
                    (cx, cy, g.to_string())
                });
                // Search hits snapshot (owned; renderer borrows below).
                let search = if self.tabs[self.active].search.on {
                    Some((
                        self.tabs[self.active].search.matches.clone(),
                        self.tabs[self.active].search.current,
                    ))
                } else {
                    None
                };
                let rail_cols = self.rail_cols();
                self.refresh_chrome();
                self.refresh_inspector();
                let vp = self.frame_layout().term;
                let gpu_vp = zet_gpu::renderer::Viewport {
                    x: vp.x,
                    y: vp.y,
                    cols: vp.cols,
                    rows: vp.rows,
                };
                let rail_snapshot = if rail_cols > 0 {
                    self.chrome.snapshot_handle()
                } else {
                    None
                };
                let inspector_vp_layout = self.frame_layout().inspector;
                let inspector_cols = inspector_vp_layout.map_or(0, |vp| vp.cols as usize);
                if let (Some(r), Some(_)) = (&mut self.renderer, &self.window) {
                    r.draw_terminal_in(&self.tabs[self.active].term, gpu_vp, sel);
                    if rail_cols > 0 {
                        if let Some(snapshot) = rail_snapshot.as_ref() {
                            let rail_vp = zet_gpu::renderer::Viewport {
                                x: 0,
                                y: bar as u32,
                                cols: rail_cols as u32,
                                rows: (self.rows.saturating_sub(bar)) as u32,
                            };
                            r.draw_fill(rail_vp, zet_gpu::quad::Rgba::rgb(0.03, 0.04, 0.07));
                            Self::draw_rail(r, rail_vp, snapshot, self.rail_sel);
                        }
                    }
                    if inspector_cols > 0 {
                        if let Some(insp_layout_vp) = inspector_vp_layout {
                            let insp_vp = zet_gpu::renderer::Viewport {
                                x: insp_layout_vp.x,
                                y: insp_layout_vp.y,
                                cols: insp_layout_vp.cols,
                                rows: insp_layout_vp.rows,
                            };
                            r.draw_fill(insp_vp, zet_gpu::quad::Rgba::rgb(0.012, 0.018, 0.032));
                            r.draw_text_row(insp_vp, 0, 0, "Inspector", zet_gpu::quad::Rgba::WHITE);
                            if let Some(selected) = self.inspector_sel {
                                let row = selected as u32 + 1;
                                if row < insp_vp.rows {
                                    r.draw_fill(
                                        zet_gpu::renderer::Viewport {
                                            x: insp_vp.x,
                                            y: insp_vp.y + row,
                                            cols: insp_vp.cols,
                                            rows: 1,
                                        },
                                        zet_gpu::quad::Rgba::rgb(0.040, 0.065, 0.125),
                                    );
                                }
                            }
                            if let Some(snapshot) = self.inspector_cache.snapshot_handle() {
                                r.draw_text_row(
                                    insp_vp,
                                    1,
                                    0,
                                    &format!("Session {}", snapshot.session),
                                    zet_gpu::quad::Rgba::rgb(0.72, 0.78, 0.86),
                                );
                                r.draw_text_row(
                                    insp_vp,
                                    2,
                                    0,
                                    &format!("State {}", snapshot.state),
                                    zet_gpu::quad::Rgba::rgb(0.72, 0.78, 0.86),
                                );
                                r.draw_text_row(
                                    insp_vp,
                                    3,
                                    0,
                                    &format!("Exit {}", snapshot.exit),
                                    zet_gpu::quad::Rgba::rgb(0.72, 0.78, 0.86),
                                );
                                r.draw_text_row(
                                    insp_vp,
                                    4,
                                    0,
                                    &format!("Cwd {}", snapshot.cwd),
                                    zet_gpu::quad::Rgba::rgb(0.72, 0.78, 0.86),
                                );
                            } else {
                                r.draw_text_row(
                                    insp_vp,
                                    2,
                                    0,
                                    "No session",
                                    zet_gpu::quad::Rgba::rgb(0.72, 0.78, 0.86),
                                );
                            }
                        }
                    }
                    if bar > 0 {
                        r.draw_tabbar_with_rail(&labels, active, cols, rail_cols);
                    }
                    if let Some((cx, cy, g)) = ghost {
                        r.draw_ghost(gpu_vp, cy, cx, &g);
                    }
                    if let Some((hits, cur)) = &search {
                        r.draw_search(&self.tabs[self.active].term, hits, *cur, gpu_vp);
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
                        if let Some((x, y)) = self.mouse_px {
                            if let Some(r) = &self.renderer {
                                let (cw, ch) = r.cell_px();
                                self.handle_mouse_click(x, y, cw, ch);
                            } else {
                                self.handle_mouse_click(x, y, 1, 1);
                            }
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        self.tabs[self.active].selecting = false;
                        if self.tabs[self.active]
                            .selection
                            .is_some_and(|s| s.is_empty())
                        {
                            self.tabs[self.active].selection = None;
                        } else if self.tabs[self.active].selection.is_some() {
                            // Select-to-copy: a finished drag lands in the
                            // clipboard with confirmation, no second gesture.
                            self.copy_selection();
                        }
                    }
                    // Right-click copies the selection (terminal convention).
                    (MouseButton::Right, ElementState::Pressed) => {
                        self.copy_selection();
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.needs_draw = true;
                // Wheel notch: 3 lines (LineDelta) or ~1 line per 20px.
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 3.0).round() as isize,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 20.0).round() as isize,
                };
                if self.tabs[self.active].term.alt_active() {
                    // App owns the screen (vim/helix): wheel becomes
                    // arrow keys, the xterm alternateScroll convention.
                    if let Some(s) = &mut self.tabs[self.active].session {
                        let seq = if lines >= 0 { b"\x1b[A" } else { b"\x1b[B" };
                        for _ in 0..lines.abs().min(10) {
                            let _ = s.write_all(seq);
                        }
                    }
                    return;
                }
                // Scrolling drops the selection (it belongs to the old view).
                self.tabs[self.active].selection = None;
                self.tabs[self.active].selecting = false;
                if lines >= 0 {
                    self.tabs[self.active].term.scroll_view_up(lines as usize);
                } else {
                    self.tabs[self.active]
                        .term
                        .scroll_view_down((-lines) as usize);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_px = Some((position.x, position.y));
                if self.tabs[self.active].selecting {
                    if let Some(cell) = self.mouse_cell() {
                        if let Some(sel) = &mut self.tabs[self.active].selection {
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

/// Open the window and run the event loop until the window closes.
///
/// `goal` is the turn to start once the shell is up: `None` opens the
/// window exactly as before (the human types), `Some(goal)` starts that
/// agent turn as soon as the active tab reaches its first prompt.
pub fn run(goal: Option<String>) -> std::io::Result<()> {
    zet_gpu::perf_mark("main");
    let (prog, hook) = shell_program();
    let shell_name = prog.clone();
    let event_loop = EventLoop::new().map_err(io_error)?;
    // Wake the loop the moment PTY bytes land: output renders instantly
    // instead of waiting for the heartbeat.
    let proxy = event_loop.create_proxy();
    let tab = Tab::spawn(&prog, hook.clone(), proxy.clone());
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
        tabs: vec![tab],
        active: 0,
        shell_name,
        prog: prog.clone(),
        hook: hook.clone(),
        wake: Some(proxy),
        mods: ModifiersState::empty(),
        cols: 80,
        rows: 24,
        layout: crate::window::layout::LayoutMode::TerminalFocus.layout(),
        focus: Focus::Terminal,
        rail_sel: 0,
        chrome: panels::ChromeCache::new(),
        inspector_sel: None,
        inspector_cache: panels::InspectorCache::new(),
        #[cfg(test)]
        chrome_query_count: 0,
        #[cfg(test)]
        chrome_build_count: 0,
        env_spec,
        startup_spec,
        known: zet_pty::KnownCommands::from_env(),
        mouse_px: None,
        last_frame: std::time::Instant::now(),
        needs_draw: true,
        perf_t0,
        perf_input: None,
        perf_frames: 0,
        cmd_t0: None,
        perf_blocks: 0,
        pending_goal: goal,
    };
    event_loop.run_app(&mut app).map_err(io_error)
}

/// winit errors are plain IO failures for us: the window could not be
/// opened, or the event loop failed.
fn io_error(e: winit::error::EventLoopError) -> std::io::Error {
    std::io::Error::other(e)
}

#[cfg(test)]
mod tests {
    use super::setup_lines;
    use std::time::{Duration, Instant};

    use zet_pty::Session;

    /// A shell like the window's: `env TERM=… <shell> -i`.
    fn spawn_shell() -> Session {
        Session::spawn(
            "env",
            &["TERM=xterm-256color", "bash", "--norc", "-i"],
            80,
            24,
        )
        .unwrap()
    }

    /// Run the shell to completion and return everything it wrote.
    fn drain_to_exit(s: &mut Session) -> String {
        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if s.try_exit().unwrap().is_some() {
                s.wait_with_output(&mut out).unwrap();
                break;
            }
            if Instant::now() > deadline {
                panic!("bash did not exit in time");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        String::from_utf8_lossy(&out).to_string()
    }

    /// Type `setup` into a fresh shell, ask it for `ZET_SESSION`, and
    /// return what it says.
    ///
    /// The value is printed inside brackets (`GOT=[…]`) so the answer
    /// cannot be confused with the shell's echo of the command line it
    /// was asked to run — the echo carries the literal `$ZET_SESSION`,
    /// and is filtered out by looking only at lines whose bracket holds
    /// no `$`.
    fn session_id_seen_by_shell(setup: &str) -> String {
        let mut s = spawn_shell();
        s.write_all(setup.as_bytes()).unwrap();
        s.write_all(b"printf 'GOT=[%s]\\n' \"$ZET_SESSION\"\nexit\n")
            .unwrap();

        let text = drain_to_exit(&mut s);
        text.lines()
            .filter_map(|l| {
                let start = l.find("GOT=[")? + "GOT=[".len();
                let rest = &l[start..];
                let end = rest.find(']')?;
                Some(rest[..end].to_string())
            })
            // The shell's own echo of the command: `$ZET_SESSION` unexpanded.
            .rfind(|v| !v.contains('$'))
            .unwrap_or_else(|| panic!("no session id echoed by the shell: {text}"))
    }

    /// The window's own setup bytes, typed into a real shell: the
    /// exported session id comes back, unchanged. This is the whole
    /// contract of `ZET_SESSION` — if the export line were malformed,
    /// every `zet blocks` inside the window would read the wrong
    /// session.
    #[test]
    fn the_exported_session_id_reaches_the_shell() {
        let setup = setup_lines("1712345678-4242", None);
        assert_eq!(session_id_seen_by_shell(&setup), "1712345678-4242");
    }

    /// With the hook in the setup (as the window really does it), the
    /// session id still survives: the hook line does not reset the
    /// environment.
    #[test]
    fn the_session_id_survives_the_hook_line() {
        let hook = format!(
            "{}/../../shell/zet-integration.bash",
            env!("CARGO_MANIFEST_DIR")
        );
        let setup = setup_lines("1712345678-4242", Some(&hook));
        assert_eq!(session_id_seen_by_shell(&setup), "1712345678-4242");
    }

    /// The export is what makes it inherited: a child process sees the
    /// session id too, which is how `zet blocks` inside the shell finds
    /// the session.
    #[test]
    fn a_child_process_inherits_the_session_id() {
        let mut s = spawn_shell();
        s.write_all(setup_lines("1712345678-4242", None).as_bytes())
            .unwrap();
        s.write_all(b"sh -c 'printf CHILD=[%s]\\n \"$ZET_SESSION\"'\nexit\n")
            .unwrap();

        let text = drain_to_exit(&mut s);
        assert!(
            text.contains("CHILD=[1712345678-4242]"),
            "child did not inherit the session id: {text}"
        );
    }
}

#[cfg(test)]
mod layout_fit_tests {
    use super::App;
    use super::Tab;

    fn make_test_tab() -> Tab {
        Tab {
            session: None,
            term: zet_term::Terminal::new(80, 24),
            shell_state: zet_pty::shell::ShellState::new(),
            session_id: 0,
            rx: None,
            dead: false,
            erase: 127,
            recorder: None,
            ghost: zet_suggest::Ghost::new(zet_blocks::Store::open_in_memory().unwrap()),
            selection: None,
            selecting: false,
            pending_fix: None,
            nl_rx: None,
            nl_wake: false,
            goal_mode: false,
            agent_busy: false,
            agent_rx: None,
            plan_rx: None,
            pending_plan: None,
            exec_req_rx: None,
            exec_res_tx: None,
            exec_pending: None,
            last_journal: Vec::new(),
            undo_rx: None,
            was_alt: false,
            search: super::SearchState {
                on: false,
                query: String::new(),
                matches: Vec::new(),
                current: 0,
            },
        }
    }

    fn make_test_app(cols: usize, rows: usize, layout: crate::window::layout::Layout) -> App {
        App {
            window: None,
            renderer: None,
            tabs: vec![make_test_tab()],
            active: 0,
            shell_name: "bash".to_string(),
            prog: "bash".to_string(),
            hook: None,
            wake: None,
            mods: winit::keyboard::ModifiersState::empty(),
            cols,
            rows,
            layout,
            focus: super::Focus::Terminal,
            rail_sel: 0,
            chrome: super::panels::ChromeCache::new(),
            inspector_sel: None,
            inspector_cache: super::panels::InspectorCache::new(),
            #[cfg(test)]
            chrome_query_count: 0,
            #[cfg(test)]
            chrome_build_count: 0,
            env_spec: None,
            startup_spec: None,
            known: zet_pty::KnownCommands::from_env(),
            mouse_px: None,
            last_frame: std::time::Instant::now(),
            needs_draw: true,
            perf_t0: None,
            perf_input: None,
            perf_frames: 0,
            cmd_t0: None,
            perf_blocks: 0,
            pending_goal: None,
        }
    }

    #[test]
    fn fit_all_uses_resolved_viewport() {
        let mut app = make_test_app(
            104,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        app.fit_all();
        assert_eq!(app.tabs[0].term.cols(), 80);
        assert_eq!(app.tabs[0].term.rows(), 24);

        app.tabs.push(make_test_tab());
        app.fit_all();
        assert_eq!(app.tabs[0].term.cols(), 80);
        assert_eq!(app.tabs[0].term.rows(), 23);
        assert_eq!(app.tabs[1].term.cols(), 80);
        assert_eq!(app.tabs[1].term.rows(), 23);

        app.tabs.pop();
        app.fit_all();
        assert_eq!(app.tabs[0].term.rows(), 24);
    }

    #[test]
    fn fit_all_terminal_focus_uses_full_window() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::TerminalFocus.layout(),
        );
        app.fit_all();
        assert_eq!(app.tabs[0].term.cols(), 80);
        assert_eq!(app.tabs[0].term.rows(), 24);
    }

    #[test]
    fn fit_all_resizes_real_sessions_and_preserves_tabs() {
        let first = zet_pty::Session::spawn("sleep", &["30"], 80, 24).unwrap();
        let second = zet_pty::Session::spawn("sleep", &["30"], 80, 24).unwrap();

        let mut app = make_test_app(
            104,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        app.tabs = vec![make_test_tab(), make_test_tab()];
        app.tabs[0].session = Some(first);
        app.tabs[1].session = Some(second);

        app.fit_all();
        for tab in &mut app.tabs {
            assert_eq!((tab.term.cols(), tab.term.rows()), (80, 23));
            if let Some(session) = &mut tab.session {
                assert_eq!(session.size().unwrap(), (80, 23));
            }
        }

        let mut removed = app.tabs.pop().unwrap();
        if let Some(session) = &mut removed.session {
            session.shutdown();
        }

        app.fit_all();
        assert_eq!(app.tabs[0].term.cols(), 80);
        assert_eq!(app.tabs[0].term.rows(), 24);
        if let Some(session) = &mut app.tabs[0].session {
            assert_eq!(session.size().unwrap(), (80, 24));
            session.shutdown();
        }
    }

    #[test]
    fn fit_all_caps_pty_dimensions() {
        let session = zet_pty::Session::spawn("sleep", &["30"], 80, 24).unwrap();
        let mut app = make_test_app(
            40000,
            1,
            crate::window::layout::LayoutMode::TerminalFocus.layout(),
        );
        app.tabs = vec![make_test_tab()];
        app.tabs[0].session = Some(session);

        app.fit_all();
        assert_eq!(app.tabs[0].term.cols(), 40000);
        assert_eq!(app.tabs[0].term.rows(), 1);
        if let Some(session) = &mut app.tabs[0].session {
            assert_eq!(session.size().unwrap(), (32767, 1));
            session.shutdown();
        }
    }

    #[test]
    fn chrome_cache_hidden_rail_does_not_query() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::TerminalFocus.layout(),
        );
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let recorder = zet_blocks::Recorder::new(store, "session-a");
        app.tabs[0].recorder = Some(recorder);
        let now = std::time::Instant::now();

        app.refresh_chrome_at(now);
        assert_eq!(app.chrome_query_count, 0);
        assert_eq!(app.chrome_build_count, 0);
        assert!(app.chrome.snapshot().is_none());
    }

    #[test]
    fn chrome_cache_visible_first_refresh_builds_once() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let recorder = zet_blocks::Recorder::new(store, "session-a");
        app.tabs[0].recorder = Some(recorder);
        let now = std::time::Instant::now();

        app.refresh_chrome_at(now);
        assert_eq!(app.chrome_query_count, 1);
        assert_eq!(app.chrome_build_count, 1);
        assert!(app.chrome.snapshot().is_some());

        app.refresh_chrome_at(now);
        assert_eq!(app.chrome_query_count, 1);
        assert_eq!(app.chrome_build_count, 1);
    }

    #[test]
    fn chrome_cache_unchanged_poll_does_not_rebuild() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let recorder = zet_blocks::Recorder::new(store, "session-a");
        app.tabs[0].recorder = Some(recorder);
        let now = std::time::Instant::now();

        app.refresh_chrome_at(now);
        let later = now + std::time::Duration::from_millis(250);
        app.refresh_chrome_at(later);
        assert_eq!(app.chrome_query_count, 2);
        assert_eq!(app.chrome_build_count, 1);
    }

    #[test]
    fn chrome_cache_new_command_rebuilds() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let recorder = zet_blocks::Recorder::new(store, "session-a");
        app.tabs[0].recorder = Some(recorder);
        let now = std::time::Instant::now();

        app.refresh_chrome_at(now);
        assert_eq!(app.chrome_query_count, 1);
        assert_eq!(app.chrome_build_count, 1);
        assert_eq!(app.chrome.snapshot().unwrap().commands.len(), 0);

        app.tabs[0]
            .recorder
            .as_ref()
            .unwrap()
            .store()
            .append(&zet_blocks::Block {
                id: 0,
                session: "session-a".to_string(),
                command: "echo hi".to_string(),
                output: "hi".to_string(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 1,
            })
            .unwrap();

        let later = now + std::time::Duration::from_millis(250);
        app.refresh_chrome_at(later);
        assert_eq!(app.chrome_query_count, 2);
        assert_eq!(app.chrome_build_count, 2);
        assert_eq!(app.chrome.snapshot().unwrap().commands.len(), 1);
    }

    #[test]
    fn chrome_cache_active_tab_switch_rebuilds() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        let store_a = zet_blocks::Store::open_in_memory().unwrap();
        store_a
            .append(&zet_blocks::Block {
                id: 0,
                session: "session-a".to_string(),
                command: "echo a".to_string(),
                output: "a".to_string(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 1,
            })
            .unwrap();
        let store_b = zet_blocks::Store::open_in_memory().unwrap();
        store_b
            .append(&zet_blocks::Block {
                id: 0,
                session: "session-b".to_string(),
                command: "echo b".to_string(),
                output: "b".to_string(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 1,
            })
            .unwrap();
        let mut tab_a = make_test_tab();
        tab_a.recorder = Some(zet_blocks::Recorder::new(store_a, "session-a"));
        let mut tab_b = make_test_tab();
        tab_b.recorder = Some(zet_blocks::Recorder::new(store_b, "session-b"));
        app.tabs = vec![tab_a, tab_b];
        app.active = 0;
        let now = std::time::Instant::now();
        app.refresh_chrome_at(now);
        assert_eq!(app.chrome.snapshot().unwrap().commands[0].command, "echo a");

        app.active = 1;
        let later = now + std::time::Duration::from_millis(250);
        app.refresh_chrome_at(later);
        assert_eq!(app.chrome.snapshot().unwrap().commands[0].command, "echo b");
    }

    #[test]
    fn chrome_cache_dirty_rebuilds() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let recorder = zet_blocks::Recorder::new(store, "session-a");
        app.tabs[0].recorder = Some(recorder);
        let now = std::time::Instant::now();
        app.refresh_chrome_at(now);
        app.chrome.mark_dirty();
        let later = now + std::time::Duration::from_millis(250);
        app.refresh_chrome_at(later);
        assert_eq!(app.chrome_build_count, 2);
    }

    #[test]
    fn production_key_dispatch_keeps_rail_focus_safe() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        app.layout.rail = true;
        app.focus = super::Focus::Rail;
        app.rail_sel = 0;

        let mut mods = winit::keyboard::ModifiersState::empty();
        mods.insert(winit::keyboard::ModifiersState::CONTROL);
        mods.insert(winit::keyboard::ModifiersState::SHIFT);
        app.handle_key_parts(
            winit::keyboard::Key::Character("m".into()),
            None,
            true,
            mods,
            None,
        );
        assert!(!app.layout.rail);

        app.layout.rail = true;
        app.focus = super::Focus::Rail;
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.focus, super::Focus::Terminal);

        app.focus = super::Focus::Rail;
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowDown),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.rail_sel, 0); // one tab

        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.focus, super::Focus::Terminal);

        app.focus = super::Focus::Rail;
        let mut ctrl = winit::keyboard::ModifiersState::empty();
        ctrl.insert(winit::keyboard::ModifiersState::CONTROL);
        app.handle_key_parts(
            winit::keyboard::Key::Character("c".into()),
            None,
            true,
            ctrl,
            None,
        );
        assert_eq!(app.focus, super::Focus::Terminal);

        app.focus = super::Focus::Rail;
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.focus, super::Focus::Terminal);

        app.focus = super::Focus::Terminal;
        app.layout.rail = false;
        let mut toggle_mods = winit::keyboard::ModifiersState::empty();
        toggle_mods.insert(winit::keyboard::ModifiersState::CONTROL);
        toggle_mods.insert(winit::keyboard::ModifiersState::SHIFT);
        app.handle_key_parts(
            winit::keyboard::Key::Character("m".into()),
            None,
            true,
            toggle_mods,
            None,
        );
        assert!(app.layout.rail);
        assert_eq!(app.focus, super::Focus::Terminal);

        app.focus = super::Focus::Rail;
        app.handle_key_parts(
            winit::keyboard::Key::Character("x".into()),
            Some("x"),
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.focus, super::Focus::Rail);
    }

    #[test]
    fn production_mouse_clicks_respect_rail_and_tabs() {
        let mut app = make_test_app(
            104,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        app.tabs.push(make_test_tab());
        app.mouse_px = Some((10.0, 100.0));
        app.handle_mouse_click(10.0, 100.0, 10, 20);
        assert_eq!(app.focus, super::Focus::Rail);
        assert_eq!(app.rail_sel, 1);

        app.mouse_px = Some((300.0, 40.0));
        app.handle_mouse_click(300.0, 40.0, 10, 20);
        assert_eq!(app.focus, super::Focus::Terminal);
        assert!(app.tabs[app.active].selection.is_some());

        app.mouse_px = Some((0.0, 0.0));
        let tabs_before = app.active;
        app.handle_mouse_click(0.0, 0.0, 10, 20);
        assert_eq!(app.active, tabs_before);
    }

    #[test]
    fn tab_bar_clicks_reject_rail_and_activate_valid_tabs() {
        let mut app = make_test_app(
            104,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        app.tabs.push(make_test_tab());
        app.active = 1;

        // Rail-area clicks never switch tabs.
        app.handle_mouse_click(10.0, 0.0, 10, 20);
        assert_eq!(app.active, 1);

        // The first valid tab segment is tab 0.
        app.handle_mouse_click(240.0, 0.0, 10, 20);
        assert_eq!(app.active, 0);

        // The second valid tab segment is tab 1.
        app.handle_mouse_click(640.0, 0.0, 10, 20);
        assert_eq!(app.active, 1);

        // Outside the window cannot switch tabs.
        app.handle_mouse_click(1040.0, 0.0, 10, 20);
        assert_eq!(app.active, 1);

        // A click above the window cannot switch tabs.
        app.handle_mouse_click(240.0, -1.0, 10, 20);
        assert_eq!(app.active, 1);

        // Zero-rail behavior remains unchanged.
        let mut zero_rail = make_test_app(
            104,
            24,
            crate::window::layout::LayoutMode::TerminalFocus.layout(),
        );
        zero_rail.tabs.push(make_test_tab());
        zero_rail.active = 1;
        zero_rail.handle_mouse_click(30.0, 0.0, 10, 20);
        assert_eq!(zero_rail.active, 0);
    }

    #[test]
    fn rail_session_rows_select_correct_sessions() {
        let mut app = make_test_app(
            104,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        app.tabs.push(make_test_tab());
        app.layout.rail = true;
        app.focus = super::Focus::Terminal;
        app.rail_sel = 0;

        app.handle_mouse_click(10.0, 80.0, 10, 20);
        assert_eq!(app.focus, super::Focus::Rail);
        assert_eq!(app.rail_sel, 0);
        assert_eq!(app.active, 0);

        app.handle_mouse_click(10.0, 100.0, 10, 20);
        assert_eq!(app.rail_sel, 1);
        assert_eq!(app.active, 0);

        app.handle_mouse_click(10.0, 20.0, 10, 20);
        assert_eq!(app.rail_sel, 1);

        app.handle_mouse_click(10.0, 200.0, 10, 20);
        assert_eq!(app.rail_sel, 1);

        app.handle_mouse_click(-1.0, 100.0, 10, 20);
        assert_eq!(app.rail_sel, 1);
    }

    #[test]
    fn rail_keyboard_dispatch_is_safe() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        app.tabs.push(make_test_tab());
        app.layout.rail = true;
        app.focus = super::Focus::Rail;
        app.rail_sel = 0;

        let mut mods = winit::keyboard::ModifiersState::empty();
        mods.insert(winit::keyboard::ModifiersState::CONTROL);
        mods.insert(winit::keyboard::ModifiersState::SHIFT);
        app.handle_key_parts(
            winit::keyboard::Key::Character("m".into()),
            None,
            true,
            mods,
            None,
        );
        assert!(!app.layout.rail);
        assert_eq!(app.focus, super::Focus::Terminal);

        app.layout.rail = true;
        app.focus = super::Focus::Rail;
        app.rail_sel = 1;
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowUp),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.rail_sel, 0);
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowDown),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.rail_sel, 1);

        app.rail_sel = 1;
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.active, 1);
        assert_eq!(app.focus, super::Focus::Terminal);

        app.focus = super::Focus::Rail;
        let mut ctrl = winit::keyboard::ModifiersState::empty();
        ctrl.insert(winit::keyboard::ModifiersState::CONTROL);
        app.handle_key_parts(
            winit::keyboard::Key::Character("c".into()),
            None,
            true,
            ctrl,
            None,
        );
        assert_eq!(app.focus, super::Focus::Terminal);

        app.focus = super::Focus::Rail;
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.focus, super::Focus::Terminal);

        app.focus = super::Focus::Terminal;
        app.layout.rail = false;
        let mut toggle_mods = winit::keyboard::ModifiersState::empty();
        toggle_mods.insert(winit::keyboard::ModifiersState::CONTROL);
        toggle_mods.insert(winit::keyboard::ModifiersState::SHIFT);
        app.handle_key_parts(
            winit::keyboard::Key::Character("m".into()),
            None,
            true,
            toggle_mods,
            None,
        );
        assert!(app.layout.rail);
        assert_eq!(app.focus, super::Focus::Terminal);

        app.focus = super::Focus::Rail;
        app.handle_key_parts(
            winit::keyboard::Key::Character("x".into()),
            Some("x"),
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.focus, super::Focus::Rail);
    }

    #[test]
    fn rail_snapshot_keeps_full_command_for_width_budgeting() {
        let mut app = make_test_app(
            104,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let command = "echo hello-from-zet-audit-with-a-much-longer-suffix";
        store
            .append(&zet_blocks::Block {
                id: 0,
                session: "session-a".to_string(),
                command: command.to_string(),
                output: String::new(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 14,
            })
            .unwrap();
        let mut tab = make_test_tab();
        tab.recorder = Some(zet_blocks::Recorder::new(store, "session-a"));
        app.tabs = vec![tab];

        let snapshot = app.build_rail_snapshot();
        assert_eq!(snapshot.commands[0].command, command);
        let row = super::panels::format_command_row(
            &snapshot.commands[0].command,
            &snapshot.commands[0].exit,
            &snapshot.commands[0].duration,
            24,
        );
        assert!(row.ends_with("0 14ms"), "{row:?}");
        assert!(row.contains('…'), "{row:?}");
        assert!(
            super::panels::text_cells(&row) <= 24,
            "{row:?} exceeds the 24-column rail"
        );
    }

    #[test]
    fn build_rail_snapshot_limits_and_isolates_commands() {
        let mut app = make_test_app(
            80,
            24,
            crate::window::layout::LayoutMode::Workspace.layout(),
        );
        let store = zet_blocks::Store::open_in_memory().unwrap();
        for i in 0..12 {
            store
                .append(&zet_blocks::Block {
                    id: 0,
                    session: "session-a".to_string(),
                    command: format!("cmd {i}"),
                    output: "output".to_string(),
                    exit_code: i,
                    cwd: "/tmp".to_string(),
                    git_branch: String::new(),
                    duration_ms: i as u64,
                })
                .unwrap();
        }
        let other_store = zet_blocks::Store::open_in_memory().unwrap();
        other_store
            .append(&zet_blocks::Block {
                id: 0,
                session: "session-b".to_string(),
                command: "other command".to_string(),
                output: "other".to_string(),
                exit_code: 7,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 9,
            })
            .unwrap();

        let mut tab_a = make_test_tab();
        tab_a.recorder = Some(zet_blocks::Recorder::new(store, "session-a"));
        let mut tab_b = make_test_tab();
        tab_b.recorder = Some(zet_blocks::Recorder::new(other_store, "session-b"));
        app.tabs = vec![tab_a, tab_b];
        app.active = 0;

        let snapshot = app.build_rail_snapshot();
        assert_eq!(snapshot.commands.len(), 8);
        assert!(snapshot.commands[0].command.contains("cmd 4"));
        assert!(snapshot.commands[7].command.contains("cmd 11"));
        assert!(snapshot
            .commands
            .iter()
            .all(|command| !command.command.contains("other")));

        app.active = 1;
        let snapshot = app.build_rail_snapshot();
        assert_eq!(snapshot.commands.len(), 1);
        assert!(snapshot.commands[0].command.contains("other command"));
    }

    #[test]
    fn inspector_click_focuses_inspector_without_terminal_selection() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.handle_mouse_click(1000.0, 100.0, 10, 20);
        assert_eq!(app.focus, super::Focus::Inspector);
        assert!(app.tabs[0].selection.is_none());
        assert!(!app.tabs[0].selecting);
    }

    #[test]
    fn terminal_click_restores_terminal_focus() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.focus = super::Focus::Inspector;
        app.handle_mouse_click(300.0, 100.0, 10, 20);
        assert_eq!(app.focus, super::Focus::Terminal);
    }

    #[test]
    fn inspector_escape_returns_to_terminal() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.focus = super::Focus::Inspector;
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.focus, super::Focus::Terminal);
    }

    #[test]
    fn inspector_toggle_is_independent_of_rail() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::TerminalFocus.layout(),
        );
        app.layout.rail = true;
        app.toggle_inspector();
        assert!(app.layout.inspector);
        assert!(app.layout.rail);
        app.toggle_inspector();
        assert!(!app.layout.inspector);
        assert!(app.layout.rail);
    }

    #[test]
    fn inspector_narrow_layout_does_not_strand_focus() {
        let mut app = make_test_app(
            40,
            24,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.focus = super::Focus::Inspector;
        app.refresh_inspector();
        assert_eq!(app.focus, super::Focus::Terminal);
        assert!(app.frame_layout().inspector.is_none());
    }

    #[test]
    fn inspector_session_label_is_not_duplicated() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.refresh_inspector();
        let snapshot = app.inspector_cache.snapshot().unwrap();
        assert_eq!(snapshot.session, "1");
    }

    #[test]
    fn inspector_click_selects_only_real_rows() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.refresh_inspector();
        app.handle_mouse_click(900.0, 5.0, 10, 20);
        assert_eq!(app.focus, super::Focus::Inspector);
        assert_eq!(app.inspector_sel, None);
        app.handle_mouse_click(900.0, 25.0, 10, 20);
        assert_eq!(app.inspector_sel, Some(0));
        app.handle_mouse_click(900.0, 85.0, 10, 20);
        assert_eq!(app.inspector_sel, Some(3));
        app.handle_mouse_click(900.0, 200.0, 10, 20);
        assert_eq!(app.focus, super::Focus::Inspector);
    }

    #[test]
    fn inspector_arrow_navigation_stays_bounded() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.focus = super::Focus::Inspector;
        app.inspector_sel = Some(0);
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowDown),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.inspector_sel, Some(1));
        app.inspector_sel = Some(3);
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowDown),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.inspector_sel, Some(0));
        app.inspector_sel = Some(0);
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowUp),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.inspector_sel, Some(3));
    }

    #[test]
    fn inspector_navigation_does_not_snap_scrollback() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.tabs[0].term.scroll_view_up(5);
        let before = app.tabs[0].term.view_offset();
        app.focus = super::Focus::Inspector;
        app.handle_key_parts(
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowDown),
            None,
            true,
            winit::keyboard::ModifiersState::empty(),
            None,
        );
        assert_eq!(app.tabs[0].term.view_offset(), before);
    }

    #[test]
    fn inspector_ctrl_c_returns_to_terminal() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.focus = super::Focus::Inspector;
        app.inspector_sel = Some(2);
        app.handle_key_parts(
            winit::keyboard::Key::Character("c".into()),
            Some("c"),
            true,
            winit::keyboard::ModifiersState::CONTROL,
            None,
        );
        assert_eq!(app.focus, super::Focus::Terminal);
        assert_eq!(app.inspector_sel, None);
    }

    #[test]
    fn inspector_tab_change_resets_selection() {
        let mut app = make_test_app(
            120,
            40,
            crate::window::layout::LayoutMode::MissionControl.layout(),
        );
        app.tabs.push(make_test_tab());
        app.focus = super::Focus::Inspector;
        app.inspector_sel = Some(2);
        app.active = 1;
        app.refresh_inspector();
        assert_eq!(app.focus, super::Focus::Inspector);
        assert_eq!(app.inspector_sel, None);
        assert_eq!(app.inspector_cache.snapshot().unwrap().active, 1);
    }
}
