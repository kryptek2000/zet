// Metered model wrapper: every chat call appends one usage line
// (~/.zet/usage.jsonl) with estimated tokens (chars/4 — providers
// report real counts in different shapes, estimates stay uniform).
// Powers `zet usage`. Logging is best-effort: a bad log never fails
// a model call. The log rotates: past 1MB only the last 4000 lines
// survive (ghost keystrokes log a line each — unbounded growth
// would become a janitor ticket).

use std::path::PathBuf;
use std::time::Instant;

use crate::{ChatModel, ChatRequest, ChatResponse, ModelError};

pub struct Metered<M: ChatModel> {
    inner: M,
    label: String,
    log: PathBuf,
}

/// Default log location: same ~/.zet state dir as blocks/journal.
pub fn default_log() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::Path::new(&home).join(".zet").join("usage.jsonl")
}

impl<M: ChatModel> Metered<M> {
    pub fn new(inner: M, label: &str, log: PathBuf) -> Self {
        Metered {
            inner,
            label: label.to_string(),
            log,
        }
    }

    /// Default log location is the free function below (associated so
    /// call sites read naturally).
    pub fn default_log() -> PathBuf {
        default_log()
    }

    fn request_chars(req: &ChatRequest) -> usize {
        let mut n = req.system.as_ref().map(|s| s.len()).unwrap_or(0);
        for m in &req.messages {
            n += m.text.len();
            n += m.tool_call_id.as_ref().map(|s| s.len()).unwrap_or(0);
            for c in &m.tool_calls {
                n += c.id.len() + c.name.len() + c.arguments.len();
            }
        }
        for t in &req.tools {
            n += t.name.len() + t.description.len() + t.parameters.to_string().len();
        }
        n
    }

    fn response_chars(res: &ChatResponse) -> usize {
        let mut n = res.text.len();
        for c in &res.tool_calls {
            n += c.id.len() + c.name.len() + c.arguments.len();
        }
        n
    }
}

/// Rotation cap: past this size only the tail survives. Best-effort
/// like the rest of logging (a concurrent append may lose a line;
/// single-user, estimates only — acceptable).
const MAX_LOG_BYTES: u64 = 1 << 20;
const KEEP_LOG_LINES: usize = 4000;

fn rotate_log(path: &std::path::Path) {
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) <= MAX_LOG_BYTES {
        return;
    }
    if let Ok(text) = std::fs::read_to_string(path) {
        // Keep the newest lines that fit in ~3/4 of the cap (bounded
        // by KEEP_LOG_LINES so a few giant lines can't wedge it).
        let mut keep: Vec<&str> = Vec::new();
        let mut bytes = 0u64;
        for line in text.lines().rev() {
            if keep.len() >= KEEP_LOG_LINES || bytes + line.len() as u64 > MAX_LOG_BYTES * 3 / 4 {
                break;
            }
            bytes += line.len() as u64 + 1;
            keep.push(line);
        }
        keep.reverse();
        if keep.len() < text.lines().count() {
            let _ = std::fs::write(path, keep.join("\n") + "\n");
        }
    }
}
impl<M: ChatModel> ChatModel for Metered<M> {
    fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, ModelError> {
        let t0 = Instant::now();
        let result = self.inner.chat(req);
        if let Ok(res) = &result {
            let int = Self::request_chars(req);
            let out = Self::response_chars(res);
            let line = serde_json::json!({
                "ts": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                "label": self.label,
                "in_chars": int,
                "out_chars": out,
                "in_est": int / 4,
                "out_est": out / 4,
                "ms": t0.elapsed().as_millis() as u64,
            });
            if let Some(parent) = self.log.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.log)
            {
                use std::io::Write as _;
                let _ = writeln!(f, "{line}");
            }
            rotate_log(&self.log);
        }
        result
    }
}
