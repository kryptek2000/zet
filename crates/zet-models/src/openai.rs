// OpenAI-compatible chat client: OpenAI, OpenRouter, Together, Groq,
// DeepSeek, xAI, self-hosted gateways — anything behind /chat/completions.
// Speaks both shapes of the gateway: blocking `chat`, and `chat_stream`
// over SSE for anything that implements `stream: true`.

use std::io::BufRead;

use serde_json::{json, Value};

use crate::stream::{SseAccumulator, SseStep};
use crate::{
    ChatModel, ChatRequest, ChatResponse, ChatStream, ModelError, Role, StreamEvent, ToolCall,
};

pub struct OpenAICompat {
    base_url: String,
    api_key: String,
    model: String,
    agent: ureq::Agent,
}

impl OpenAICompat {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        OpenAICompat {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            agent: ureq::Agent::new_with_config(
                ureq::config::Config::builder()
                    .http_status_as_error(false)
                    .build(),
            ),
        }
    }

    fn role(role: Role) -> &'static str {
        match role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }

    /// `messages` + `tools` in the OpenAI shape. Shared by the blocking
    /// and the streaming call: the wire body is the same either way, only
    /// `stream` differs.
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

    fn post(&self, body: Value) -> Result<ureq::http::Response<ureq::Body>, ModelError> {
        self.agent
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .send_json(body)
            .map_err(|e| ModelError::Network(e.to_string()))
    }
}

/// Provider status -> typed error, shared by chat, the stream and the
/// model list. `what` names the call in the "everything else" branch —
/// a bad status is nearly always a request we shaped wrong.
/// 401/403 credentials, 429 quota, 5xx their fault.
pub(crate) fn status_error(status: u16, snippet: String, what: &str) -> ModelError {
    match status {
        401 | 403 => ModelError::Auth(format!("provider rejected the credentials: {snippet}")),
        429 => ModelError::RateLimited(format!("provider rate limit hit: {snippet}")),
        500.. => ModelError::Server(format!("provider status {status}: {snippet}")),
        _ => ModelError::Protocol(format!("{what}: status {status}: {snippet}")),
    }
}

/// First 300 chars of an error body, single-lined. Provider errors name
/// the real cause (unknown model, bad field); bare statuses don't.
/// JSON error envelopes (`{"error":{"message":…}}`, OpenRouter-style)
/// collapse to just the message — the envelope hides the diagnosis.
pub(crate) fn snippet(body: &mut ureq::Body) -> String {
    let text = body.read_to_string().unwrap_or_default();
    if let Some(msg) = error_message(&text) {
        return msg;
    }
    flatten(&text)
}

/// Pull `.error.message` / `.message` out of a JSON error body.
/// None when the body isn't that shape (falls back to raw snippet).
/// Prefixed with `.error.type` when present — callers match on it
/// (the Go gateway's `ModelProtocolUnsupported` drives fallback).
pub(crate) fn error_message(text: &str) -> Option<String> {
    let json: Value = serde_json::from_str(text).ok()?;
    let msg = json
        .pointer("/error/message")
        .or_else(|| json.pointer("/message"))?
        .as_str()?;
    let flat = flatten(msg);
    if flat.is_empty() {
        return None;
    }
    let kind = json
        .pointer("/error/type")
        .and_then(|t| t.as_str())
        .unwrap_or("");
    if kind.is_empty() {
        Some(flat)
    } else {
        Some(format!("{kind}: {flat}"))
    }
}

pub(crate) fn flatten(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut short = flat.chars().take(300).collect::<String>();
    if short.len() < flat.len() {
        short.push('…');
    }
    short
}

impl ChatModel for OpenAICompat {
    fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, ModelError> {
        let mut resp = self.post(self.chat_body(req))?;
        // Provider error bodies carry the real reason (bad model id, bad
        // payload shape); surface a snippet instead of a bare status.
        let status = resp.status().as_u16();
        if status != 200 {
            let snippet = snippet(resp.body_mut());
            return Err(status_error(status, snippet, "provider rejected request"));
        }
        let payload: Value = resp
            .body_mut()
            .read_json()
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
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }
        Ok(ChatResponse { text, tool_calls })
    }
}

impl ChatStream for OpenAICompat {
    /// SSE: same body plus `"stream": true`, then `data:` frames read
    /// line by line until `[DONE]`. Deltas go out through `on_event` as
    /// they arrive; the returned response is the whole assembled answer,
    /// so a streaming caller and a blocking caller see the same thing.
    fn chat_stream(
        &self,
        req: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<ChatResponse, ModelError> {
        let mut body = self.chat_body(req);
        body["stream"] = json!(true);
        let mut resp = self.post(body)?;
        let status = resp.status().as_u16();
        if status != 200 {
            let snippet = snippet(resp.body_mut());
            return Err(status_error(status, snippet, "provider rejected request"));
        }

        let mut acc = SseAccumulator::new();
        let mut lines = std::io::BufReader::new(resp.body_mut().as_reader());
        let mut line = String::new();
        loop {
            line.clear();
            let n = lines
                .read_line(&mut line)
                .map_err(|e| ModelError::Network(e.to_string()))?;
            // Connection closed without [DONE]: keep what arrived. A
            // half-answer plus a truncation error would hide the text
            // the user already paid for.
            if n == 0 {
                break;
            }
            match acc.push_line(&line)? {
                SseStep::Events(events) => {
                    for event in events {
                        on_event(event);
                    }
                }
                SseStep::Done => break,
            }
        }
        on_event(StreamEvent::Done);
        acc.finish()
    }
}

/// `GET {base}/models` ids (OpenAI shape: `{"data":[{"id":…}]}`).
/// Powers `zet models`: OpenRouter, Go, any compat endpoint. 10s bound.
pub fn list_models(base_url: &str, api_key: &str) -> Result<Vec<String>, crate::ModelError> {
    use crate::ModelError;
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .build(),
    );
    let mut resp = agent
        .get(format!("{}/models", base_url.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {api_key}"))
        .call()
        .map_err(|e| ModelError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    if status != 200 {
        let snippet = snippet(resp.body_mut());
        return Err(status_error(status, snippet, "model list failed"));
    }
    let payload: Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| ModelError::Protocol(format!("bad model list JSON: {e}")))?;
    let ids = payload
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(ids)
}

/// OpenRouter free-tier marker: ids ending in `:free`.
pub fn is_free_model(id: &str) -> bool {
    id.ends_with(":free")
}
