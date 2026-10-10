// OpenAI-compatible chat client: OpenAI, OpenRouter, Together, Groq,
// DeepSeek, xAI, self-hosted gateways — anything behind /chat/completions.

use serde_json::{json, Value};

use crate::{ChatModel, ChatRequest, ChatResponse, ModelError, Role, ToolCall};

pub struct OpenAICompat {
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAICompat {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        OpenAICompat {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
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
}

impl ChatModel for OpenAICompat {
    fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, ModelError> {
        let mut messages = Vec::with_capacity(req.messages.len() + 1);
        if let Some(system) = &req.system {
            messages.push(json!({"role": "system", "content": system}));
        }
        for m in &req.messages {
            messages.push(json!({"role": Self::role(m.role), "content": m.text}));
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
        let body = json!({
            "model": self.model,
            "messages": messages,
            "tools": tools,
            "max_tokens": req.max_tokens,
        });

        let mut resp = ureq::post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| match e {
                ureq::Error::StatusCode(401) => {
                    ModelError::Auth("provider rejected the API key".to_string())
                }
                ureq::Error::StatusCode(429) => {
                    ModelError::RateLimited("provider rate limit hit".to_string())
                }
                ureq::Error::StatusCode(code) if code >= 500 => {
                    ModelError::Server(format!("provider status {code}"))
                }
                other => ModelError::Network(other.to_string()),
            })?;
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
