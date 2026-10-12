//! OpenCode Go gateway client: session headers + per-model protocol.
//!
//! Two gateway quirks, both observed live:
//! - chat/completions requires `x-opencode-session*` headers (the CLI
//!   sends its session id; we mint `ses_` ids per client). The models
//!   listing needs none.
//! - models differ in protocol: some serve /chat/completions, others
//!   (muse-spark) only /responses (`ModelProtocolUnsupported`
//!   otherwise). We try chat first and fall back, caching the winner
//!   per model for the process.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::{ChatModel, ChatRequest, ChatResponse, ModelError, Role};

/// The Go gateway base; the opencode: provider pins this.
pub const GO_BASE_URL: &str = "https://opencode.ai/zen/go/v1";

/// Winner cache: model -> true when it needs /responses.
static PROTOCOL_CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();

fn cached_responses(model: &str) -> bool {
    PROTOCOL_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map(|m| m.get(model).copied().unwrap_or(false))
        .unwrap_or(false)
}

fn cache_responses(model: &str) {
    let lock = PROTOCOL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut m) = lock.lock() {
        m.insert(model.to_string(), true);
    }
}

/// Minted affinity id (`ses_` + 32 random bytes). Uniqueness only —
/// the gateway attributes usage to it; nothing secret derives from it.
pub fn new_session_id() -> String {
    let mut bytes = [0u8; 32];
    let filled = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| {
            use std::io::Read as _;
            f.read_exact(&mut bytes)
        })
        .is_ok();
    if !filled {
        // Fallback: pid + nanos + counter hashed (unique, not secret).
        use std::hash::{DefaultHasher, Hash, Hasher as _};
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let mut h = DefaultHasher::new();
        std::process::id().hash(&mut h);
        std::time::SystemTime::now().hash(&mut h);
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .hash(&mut h);
        let x = h.finish();
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = ((x >> ((i % 8) * 8)) & 0xff) as u8;
        }
    }
    let mut s = String::with_capacity(68);
    s.push_str("ses_");
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub struct GoClient {
    base_url: String,
    api_key: String,
    model: String,
    agent: ureq::Agent,
    session: String,
}

impl GoClient {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        GoClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            agent: ureq::Agent::new_with_config(
                ureq::config::Config::builder()
                    .http_status_as_error(false)
                    .build(),
            ),
            session: new_session_id(),
        }
    }

    fn post(&self, path: &str, body: Value) -> Result<(u16, String), ModelError> {
        let mut resp = self
            .agent
            .post(format!("{}{path}", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("x-opencode-session", &self.session)
            .header("x-opencode-session-id", &self.session)
            .header("x-session-affinity", &self.session)
            .header("X-Session-Id", &self.session)
            .send_json(body)
            .map_err(|e| ModelError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        Ok((status, text))
    }

    fn role(role: Role) -> &'static str {
        match role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }

    fn chat_body(&self, req: &ChatRequest) -> Value {
        let mut messages = Vec::with_capacity(req.messages.len() + 1);
        if let Some(system) = &req.system {
            messages.push(json!({"role": "system", "content": system}));
        }
        for m in &req.messages {
            let mut msg = serde_json::Map::new();
            msg.insert(
                "role".to_string(),
                Value::String(Self::role(m.role).to_string()),
            );
            msg.insert("content".to_string(), Value::String(m.text.clone()));
            if let Some(id) = &m.tool_call_id {
                msg.insert("tool_call_id".to_string(), Value::String(id.clone()));
            }
            if !m.tool_calls.is_empty() {
                let calls: Vec<Value> = m
                    .tool_calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {"name": c.name, "arguments": c.arguments},
                        })
                    })
                    .collect();
                msg.insert("tool_calls".to_string(), Value::Array(calls));
            }
            messages.push(Value::Object(msg));
        }
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        json!({
            "model": self.model,
            "messages": messages,
            "tools": tools,
            "max_tokens": req.max_tokens,
        })
    }

    fn fail(status: u16, text: &str) -> ModelError {
        let snippet =
            super::openai::error_message(text).unwrap_or_else(|| super::openai::flatten(text));
        match status {
            401 | 403 => ModelError::Auth(format!("provider rejected the credentials: {snippet}")),
            429 => ModelError::RateLimited(format!("provider rate limit hit: {snippet}")),
            500.. => ModelError::Server(format!("provider status {status}: {snippet}")),
            _ => ModelError::Protocol(format!(
                "provider rejected request: status {status}: {snippet}"
            )),
        }
    }

    fn chat_try(&self, req: &ChatRequest) -> Result<ChatResponse, ModelError> {
        let (status, text) = self.post("/chat/completions", self.chat_body(req))?;
        if status != 200 {
            return Err(Self::fail(status, &text));
        }
        let payload: Value = serde_json::from_str(&text)
            .map_err(|e| ModelError::Protocol(format!("bad response JSON: {e}")))?;
        let message = payload
            .pointer("/choices/0/message")
            .ok_or_else(|| ModelError::Protocol("missing choices[0].message".to_string()))?;
        let text = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();
        let mut tool_calls = Vec::new();
        if let Some(calls) = message.get("tool_calls").and_then(|c| c.as_array()) {
            for call in calls {
                let id = call
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let function = call.get("function").cloned().unwrap_or(Value::Null);
                let name = function
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let arguments = function
                    .get("arguments")
                    .map(|v| {
                        if let Some(s) = v.as_str() {
                            s.to_string()
                        } else {
                            v.to_string()
                        }
                    })
                    .unwrap_or_default();
                tool_calls.push(crate::ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }
        Ok(ChatResponse { text, tool_calls })
    }

    /// Flatten messages to one input (system first). NL turns (no tools,
    /// no call history) stay a plain string; agent turns become an item
    /// array carrying `function_call` / `function_call_output` linkage —
    /// verified live against the Go gateway, which returns `call_id`
    /// (not `id`) on its function_call items.
    /// Reasoning effort stays minimal (answers are short; the model
    /// would otherwise burn the token budget thinking) with a 512 floor
    /// so thinking never starves the answer.
    fn responses_body(&self, req: &ChatRequest) -> Result<Value, ModelError> {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();
        let mut body = json!({
            "model": self.model,
            "max_output_tokens": req.max_tokens.max(512),
            "reasoning": {"effort": "minimal"},
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        let needs_items = !req.tools.is_empty()
            || req
                .messages
                .iter()
                .any(|m| !m.tool_calls.is_empty() || m.tool_call_id.is_some());
        if needs_items {
            let mut items = Vec::new();
            if let Some(system) = &req.system {
                items.push(json!({"role": "system", "content": system}));
            }
            for m in &req.messages {
                match m.role {
                    Role::Tool => {
                        // Call linkage is the whole protocol: a result with
                        // no call id answers nothing — loud, not null.
                        let Some(call_id) = &m.tool_call_id else {
                            return Err(ModelError::Protocol(
                                "tool result without call_id has no call to answer".to_string(),
                            ));
                        };
                        items.push(json!({
                            "type": "function_call_output",
                            "call_id": call_id,
                            "output": m.text,
                        }));
                    }
                    _ => {
                        if !m.text.is_empty() {
                            items.push(json!({"role": Self::role(m.role), "content": m.text}));
                        }
                        for c in &m.tool_calls {
                            items.push(json!({
                                "type": "function_call",
                                "call_id": c.id,
                                "name": c.name,
                                "arguments": c.arguments,
                            }));
                        }
                    }
                }
            }
            body["input"] = Value::Array(items);
        } else {
            let mut parts = Vec::new();
            if let Some(system) = &req.system {
                parts.push(system.clone());
            }
            for m in &req.messages {
                parts.push(format!("{}: {}", Self::role(m.role), m.text));
            }
            body["input"] = Value::String(parts.join("\n"));
        }
        Ok(body)
    }

    fn responses_try(&self, req: &ChatRequest) -> Result<ChatResponse, ModelError> {
        let (status, text) = self.post("/responses", self.responses_body(req)?)?;
        if status != 200 {
            return Err(Self::fail(status, &text));
        }
        let payload: Value = serde_json::from_str(&text)
            .map_err(|e| ModelError::Protocol(format!("bad response JSON: {e}")))?;
        let mut out_text = String::new();
        let mut tool_calls = Vec::new();
        if let Some(items) = payload.get("output").and_then(|o| o.as_array()) {
            for item in items {
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("function_call") => {
                        let id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("id").and_then(|v| v.as_str()))
                            .unwrap_or_default()
                            .to_string();
                        let name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let arguments = item
                            .get("arguments")
                            .map(|v| {
                                if let Some(s) = v.as_str() {
                                    s.to_string()
                                } else {
                                    v.to_string()
                                }
                            })
                            .unwrap_or_default();
                        tool_calls.push(crate::ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    _ => {
                        if let Some(contents) = item.get("content").and_then(|c| c.as_array()) {
                            for part in contents {
                                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                                    out_text.push_str(t);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(ChatResponse {
            text: out_text,
            tool_calls,
        })
    }
}

/// True when the error is the gateway's wrong-protocol signal.
fn is_protocol_unsupported(err: &ModelError) -> bool {
    matches!(err, ModelError::Protocol(s) if s.contains("ModelProtocolUnsupported"))
}

impl ChatModel for GoClient {
    fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, ModelError> {
        if !cached_responses(&self.model) {
            match self.chat_try(req) {
                Ok(r) => return Ok(r),
                Err(e) if is_protocol_unsupported(&e) => cache_responses(&self.model),
                Err(e) => return Err(e),
            }
        }
        self.responses_try(req)
    }
}
