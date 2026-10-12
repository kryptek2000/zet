// zet-context — what the model gets to see: recent terminal state,
// scrubbed of secrets, capped by budget. See docs/ZET-DESIGN.md §8 (M1).

use std::sync::OnceLock;
use zet_blocks::Block;

/// Default render budget (chars). Enough for ~10 recent steps; the
/// caller (agent turn, NL request) pays it once per request.
pub const DEFAULT_BUDGET: usize = 6000;
/// Per-output cap: full logs stay in `zet blocks`, not in the prompt.
pub const PER_OUTPUT_CAP: usize = 800;
/// How many recent blocks to consider before the budget cuts in.
const MAX_ENTRIES: usize = 12;

pub struct GitState {
    pub branch: String,
    pub dirty: usize,
}

struct Entry {
    command: String,
    exit_code: i32,
    output: String,
}

pub struct Context {
    cwd: String,
    git: Option<GitState>,
    entries: Vec<Entry>,
}

fn patterns() -> &'static Vec<(regex::Regex, &'static str)> {
    static P: OnceLock<Vec<(regex::Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        vec![
            // OpenAI-style secret keys.
            (regex::Regex::new(r"sk-[A-Za-z0-9\-_]{16,}").unwrap(), "[REDACTED]"),
            // GitHub tokens.
            (
                regex::Regex::new(r"(ghp|gho|ghu|ghs|ghr|github_pat)_[A-Za-z0-9_]{10,}").unwrap(),
                "[REDACTED]",
            ),
            // AWS access keys.
            (regex::Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(), "[REDACTED]"),
            // Slack tokens.
            (regex::Regex::new(r"xox[bpas]-[A-Za-z0-9\-]+").unwrap(), "[REDACTED]"),
            // Bearer JWTs.
            (
                regex::Regex::new(r"Bearer\s+[A-Za-z0-9\-_]+\.[A-Za-z0-9\-_]+\.[A-Za-z0-9\-_]+")
                    .unwrap(),
                "Bearer [REDACTED]",
            ),
            // PEM private-key armor (headers, not bodies: base64 bodies
            // are indistinguishable from hashes, which must survive).
            (
                regex::Regex::new(r"-----[A-Z ]*PRIVATE KEY-----").unwrap(),
                "[REDACTED]",
            ),
            // Named secrets: password=..., api_key: "...", token='...'.
            // Value only — the name tells the model what was redacted.
            // (Raw string: a literal quote would end it, so match quotes
            // via hex escapes instead.)
            (
                regex::Regex::new(
                    r"(?i)(api[_-]?key|api[_-]?token|access[_-]?token|secret|password|bearer)\s*[:=]\s*[\x22\x27]?\S+",
                )
                .unwrap(),
                "$1=[REDACTED]",
            ),
        ]
    })
}

/// Redact secrets from text bound for a model. Tight patterns over
/// loud ones: a git sha or the word "password" alone must survive.
pub fn scrub(text: &str) -> String {
    let mut out = text.to_string();
    for (re, replacement) in patterns() {
        out = re.replace_all(&out, *replacement).into_owned();
    }
    out
}

fn truncate_chars(text: &str, cap: usize) -> String {
    let mut out: String = text.chars().take(cap).collect();
    if out.len() < text.len() {
        out.push('…');
    }
    out
}

/// Best-effort repo state for terminal context: branch + dirty count.
/// None outside repos or when git fails. Fast local calls; failures
/// are silent (context degrades, the turn still runs).
pub fn git_state(cwd: &std::path::Path) -> Option<GitState> {
    let branch = git_out(cwd, &["branch", "--show-current"])?;
    if branch.is_empty() {
        return None;
    }
    let status = git_out(cwd, &["status", "--porcelain"])?;
    Some(GitState {
        branch,
        dirty: status.lines().count(),
    })
}

fn git_out(cwd: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Recent terminal state, scrubbed and budgeted, newest entries kept.
pub fn gather(blocks: &[Block], cwd: &str, git: Option<GitState>, budget: usize) -> Context {
    let mut entries = Vec::new();
    // Reserve a line each for cwd/git; the rest goes to entries.
    let mut spent = cwd.len() + 64;
    for b in blocks.iter().rev().take(MAX_ENTRIES) {
        let command = scrub(&b.command);
        let output = truncate_chars(&scrub(&b.output), PER_OUTPUT_CAP);
        let cost = command.len() + output.len() + 32;
        if spent + cost > budget {
            break;
        }
        spent += cost;
        entries.push(Entry {
            command,
            exit_code: b.exit_code,
            output,
        });
    }
    entries.reverse();
    Context {
        cwd: cwd.to_string(),
        git,
        entries,
    }
}

impl Context {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.git.is_none()
    }

    pub fn render(&self) -> String {
        let mut out = format!("cwd: {}\n", self.cwd);
        if let Some(g) = &self.git {
            out.push_str(&format!("git: {} (dirty: {})\n", g.branch, g.dirty));
        }
        for e in &self.entries {
            out.push_str(&format!("$ {} (exit {})\n", e.command, e.exit_code));
            let trimmed = e.output.trim_end();
            if !trimmed.is_empty() {
                out.push_str(trimmed);
                out.push('\n');
            }
        }
        out
    }
}
