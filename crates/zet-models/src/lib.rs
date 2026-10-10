use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod mcp;
pub mod openai;

pub use mcp::McpError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool input.
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON arguments.
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    Auth(String),
    RateLimited(String),
    Server(String),
    Network(String),
    Protocol(String),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::Auth(e) => write!(f, "auth: {e}"),
            ModelError::RateLimited(e) => write!(f, "rate limited: {e}"),
            ModelError::Server(e) => write!(f, "server: {e}"),
            ModelError::Network(e) => write!(f, "network: {e}"),
            ModelError::Protocol(e) => write!(f, "protocol: {e}"),
        }
    }
}

impl std::error::Error for ModelError {}

pub trait ChatModel {
    fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, ModelError>;
}

/// Provider configuration. Keys arrive from the OS keychain at runtime
/// (see `zet auth add`); they are never written to disk by this crate.
#[derive(Debug, Clone)]
pub enum Provider {
    OpenAICompat {
        base_url: String,
        api_key: String,
        model: String,
    },
    Local {
        endpoint: String,
        model: String,
    },
}

impl Provider {
    /// OpenRouter is one line: base URL + key + any vendor/model id.
    pub fn openrouter(api_key: String, model: String) -> Self {
        Provider::OpenAICompat {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key,
            model,
        }
    }

    pub fn redacted(&self) -> HashMap<&'static str, String> {
        let mut m = HashMap::new();
        match self {
            Provider::OpenAICompat {
                base_url, model, ..
            } => {
                m.insert("base_url", base_url.clone());
                m.insert("model", model.clone());
                m.insert("api_key", "***".to_string());
            }
            Provider::Local { endpoint, model } => {
                m.insert("endpoint", endpoint.clone());
                m.insert("model", model.clone());
            }
        }
        m
    }
}
