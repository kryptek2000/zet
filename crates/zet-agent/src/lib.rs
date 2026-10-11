// Single-lane agent: one shell step per model turn, every step gated by
// zet-policy tiers. Tool output is data (Tool-role messages), never parsed
// as instructions — prompt injection via command output cannot become a step.

use std::path::{Path, PathBuf};
use zet_models::{ChatModel, ChatRequest, Message, ModelError, Role, ToolDef};
use zet_policy::{classify, decide, Approval, Tier};

pub mod journal;
pub use journal::{append_journal, journal_path_default, read_journal};

pub const SYSTEM_PROMPT: &str = "You are ZET, a terminal agent. Single lane: \
    request exactly ONE run_shell call per turn. Smallest step that advances \
    the goal. Read before writing. Never repeat a denied command.";

pub fn shell_tool_def() -> ToolDef {
    ToolDef {
        name: "run_shell".to_string(),
        description: "Run one shell command, output returned as text.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string"}},
            "required": ["command"],
        }),
    }
}

/// Runs an approved shell step. Returns (exit code, output text).
/// DirectExec spawns hidden `sh -c` (default); window shells inject a
/// PTY executor so every agent action is typed visibly and recorded.
pub trait Executor: Send {
    fn exec(&mut self, command: &str, workdir: &Path) -> (Option<i32>, String);
}

pub struct DirectExec;

impl Executor for DirectExec {
    fn exec(&mut self, command: &str, workdir: &Path) -> (Option<i32>, String) {
        let out = std::process::Command::new("sh")
            .current_dir(workdir)
            .args(["-c", command])
            .output();
        match out {
            Ok(o) => {
                let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
                let err = String::from_utf8_lossy(&o.stderr);
                if !err.is_empty() {
                    text.push_str(&err);
                }
                text.push_str(&format!("[exit {}]", o.status.code().unwrap_or(-1)));
                (o.status.code(), text)
            }
            Err(e) => (None, format!("exec failed: {e}")),
        }
    }
}

pub trait Approver {
    /// Called for Ask / ConfirmType steps. Return true to proceed.
    fn approve(&self, tier: Tier, command: &str) -> bool;
}

/// Approves everything (tests, non-interactive T0/T1 flows).
pub struct AllowAll;

impl Approver for AllowAll {
    fn approve(&self, _tier: Tier, _command: &str) -> bool {
        true
    }
}

/// Approves reads only; denies every write (red-team default).
pub struct DenyNonRead;

impl Approver for DenyNonRead {
    fn approve(&self, tier: Tier, _command: &str) -> bool {
        tier == Tier::T0
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JournalEntry {
    pub command: String,
    pub tier: Tier,
    pub approval: Approval,
    pub approved: bool,
    pub executed: bool,
    pub exit_code: Option<i32>,
    /// Raw step output, verbatim. M0: no redaction — secret scrubbing before
    /// model upload is zet-context's job (M1); never ship the journal off-box.
    pub output: String,
    /// Pre-mutation checkpoint (Some when a T1+ step ran in a repo).
    pub checkpoint: Option<Checkpoint>,
}

#[derive(Debug)]
pub enum AgentError {
    Model(ModelError),
    MaxIters(usize),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::Model(e) => write!(f, "model: {e}"),
            AgentError::MaxIters(n) => write!(f, "no answer after {n} tool turns"),
        }
    }
}

impl std::error::Error for AgentError {}

pub struct Agent<M: ChatModel, A: Approver> {
    pub model: M,
    approver: A,
    in_repo: bool,
    max_iters: usize,
    workdir: PathBuf,
    executor: Box<dyn Executor>,
    pub journal: Vec<JournalEntry>,
}

impl<M: ChatModel, A: Approver> Agent<M, A> {
    pub fn new(model: M, approver: A, in_repo: bool, max_iters: usize) -> Self {
        Agent {
            model,
            approver,
            in_repo,
            max_iters,
            workdir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            executor: Box::new(DirectExec),
            journal: Vec::new(),
        }
    }

    pub fn with_executor<E: Executor + 'static>(mut self, executor: E) -> Self {
        self.executor = Box::new(executor);
        self
    }

    pub fn with_workdir(mut self, dir: PathBuf) -> Self {
        self.workdir = dir;
        self
    }

    /// Snapshot the workdir repo before a mutation. `None` when not a repo;
    /// in a repo always `Some` (HEAD is recorded even for clean trees).
    pub fn checkpoint(&self) -> Result<Option<Checkpoint>, UndoError> {
        if !detect_repo(&self.workdir) {
            return Ok(None);
        }
        let head = git(&self.workdir, &["rev-parse", "HEAD"])?;
        if git(&self.workdir, &["status", "--porcelain"])?.is_empty() {
            return Ok(Some(Checkpoint { head, stash: None }));
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let message = format!("zet-cp-{}-{nanos}", std::process::id());
        git(&self.workdir, &["stash", "push", "-u", "-m", &message])?;
        let list = git(&self.workdir, &["stash", "list"])?;
        if list.contains(&message) {
            Ok(Some(Checkpoint {
                head,
                stash: Some(message),
            }))
        } else {
            Ok(Some(Checkpoint { head, stash: None }))
        }
    }

    /// One-key undo: restore a checkpoint into the workdir. `None` is a no-op.
    /// Drops the step's own mutations first (reset to the recorded HEAD),
    /// then pops the stash when the tree was dirty at checkpoint time.
    /// Assumes the user hasn't hand-edited files between step and undo.
    pub fn undo(&self, cp: &Option<Checkpoint>) -> Result<(), UndoError> {
        let Some(cp) = cp else {
            return Ok(());
        };
        git(&self.workdir, &["reset", "--hard", "--quiet", &cp.head])?;
        git(&self.workdir, &["clean", "-fd", "--quiet"])?;
        if let Some(message) = &cp.stash {
            let list = git(&self.workdir, &["stash", "list", "--format=%gd %s"])?;
            let entry = list
                .lines()
                .find(|l| l.contains(message))
                .ok_or_else(|| UndoError::NotFound(message.clone()))?;
            let rev = entry.split_whitespace().next().unwrap_or("stash@{0}");
            git(&self.workdir, &["stash", "pop", "--quiet", rev])?;
        }
        Ok(())
    }

    pub fn run(&mut self, goal: &str) -> Result<String, AgentError> {
        let mut messages = vec![Message {
            role: Role::User,
            text: goal.to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }];
        for _ in 0..self.max_iters {
            let req = ChatRequest {
                model: String::new(),
                system: Some(SYSTEM_PROMPT.to_string()),
                messages: messages.clone(),
                tools: vec![shell_tool_def()],
                max_tokens: 1024,
            };
            let res = self.model.chat(&req).map_err(AgentError::Model)?;
            // The assistant turn is always recorded, calls included: providers
            // reject tool messages that answer no preceding tool_calls.
            if !res.text.is_empty() || !res.tool_calls.is_empty() {
                messages.push(Message {
                    role: Role::Assistant,
                    text: res.text.clone(),
                    tool_call_id: None,
                    tool_calls: res.tool_calls.clone(),
                });
            }
            if res.tool_calls.is_empty() {
                return Ok(res.text);
            }
            // Single lane: first call runs this turn, the rest are deferred.
            let first = &res.tool_calls[0];
            let mut results = Vec::with_capacity(res.tool_calls.len());
            results.push(self.step(first));
            for deferred in &res.tool_calls[1..] {
                results.push((
                    deferred.id.clone(),
                    "deferred: one shell step per turn; re-request next turn.".to_string(),
                ));
            }
            for (id, text) in results {
                messages.push(Message {
                    role: Role::Tool,
                    text,
                    tool_call_id: Some(id),
                    tool_calls: vec![],
                });
            }
        }
        Err(AgentError::MaxIters(self.max_iters))
    }

    fn step(&mut self, call: &zet_models::ToolCall) -> (String, String) {
        if call.name != "run_shell" {
            return (
                call.id.clone(),
                format!("error: unknown tool '{}'", call.name),
            );
        }
        let command = match serde_json::from_str::<serde_json::Value>(&call.arguments) {
            Ok(v) => match v.get("command").and_then(|c| c.as_str()) {
                Some(c) => c.to_string(),
                None => {
                    return (
                        call.id.clone(),
                        "error: missing 'command' string".to_string(),
                    );
                }
            },
            Err(_) => {
                return (call.id.clone(), "error: arguments are not JSON".to_string());
            }
        };
        let tier = classify(&command);
        let approval = decide(tier, self.in_repo);
        let approved = match approval {
            Approval::Auto => true,
            Approval::Ask | Approval::ConfirmType => self.approver.approve(tier, &command),
        };
        if !approved {
            self.journal.push(JournalEntry {
                command: command.clone(),
                tier,
                approval,
                approved: false,
                executed: false,
                exit_code: None,
                output: format!("denied ({approval:?}): {command}"),
                checkpoint: None,
            });
            return (
                call.id.clone(),
                format!("denied by policy ({tier:?}): command was not executed"),
            );
        }
        // Checkpoint every mutation: snapshot the repo before T1+ writes so
        // one-key undo can restore. A failed checkpoint blocks the step.
        let checkpoint = if tier != Tier::T0 && self.in_repo {
            match self.checkpoint() {
                Ok(cp) => cp,
                Err(e) => {
                    self.journal.push(JournalEntry {
                        command: command.clone(),
                        tier,
                        approval,
                        approved: true,
                        executed: false,
                        exit_code: None,
                        output: format!("checkpoint failed, step blocked: {e}"),
                        checkpoint: None,
                    });
                    return (
                        call.id.clone(),
                        format!("checkpoint failed, step blocked: {e}"),
                    );
                }
            }
        } else {
            None
        };
        let (exit_code, output) = self.executor.exec(&command, &self.workdir);
        self.journal.push(JournalEntry {
            command: command.clone(),
            tier,
            approval,
            approved: true,
            executed: true,
            exit_code,
            output: output.clone(),
            checkpoint,
        });
        (call.id.clone(), output)
    }
}

/// Pre-mutation snapshot: the repo HEAD plus a `git stash` entry when the
/// tree was dirty (`stash -u` covers untracked). Clean trees record HEAD
/// only. Restored by [`Agent::undo`], which the user invokes explicitly —
/// undo of a clean-tree checkpoint runs `reset --hard` + `clean -fd`
/// scoped to the workdir.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Checkpoint {
    head: String,
    stash: Option<String>,
}

#[derive(Debug)]
pub enum UndoError {
    Git(String),
    NotFound(String),
}

impl std::fmt::Display for UndoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UndoError::Git(e) => write!(f, "git: {e}"),
            UndoError::NotFound(e) => write!(f, "checkpoint not found: {e}"),
        }
    }
}

impl std::error::Error for UndoError {}

fn git(cwd: &Path, args: &[&str]) -> Result<String, UndoError> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| UndoError::Git(e.to_string()))?;
    if !out.status.success() {
        return Err(UndoError::Git(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True when `cwd` is inside a git work tree.
pub fn detect_repo(cwd: &std::path::Path) -> bool {
    std::process::Command::new("git")
        .args([
            "-C",
            &cwd.to_string_lossy(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
}
