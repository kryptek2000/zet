// MCP tool registry (docs/ZET-DESIGN.md §8): one spawned stdio server per
// name, every server tool surfaced to the gateway as
// `mcp__{server}__{tool}` (`PREFIX`), and a `zet.mcp.toml` loader. The
// JSON-RPC client itself lives in `zet_models::mcp`; this crate is the
// host side that fans out to N of them and routes a model's tool call
// back to the server that owns it.
//
// Keys are never involved here: servers are subprocesses started from the
// user's own config, and the agent talks to them over pipes.

use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};

use zet_models::mcp::McpClient;
pub use zet_models::mcp::McpError;
pub use zet_models::mcp::PREFIX;
use zet_models::{ToolCall, ToolDef};

/// One spawned MCP server.
struct Server {
    name: String,
    client: McpClient,
}

/// Every configured MCP server, in the order they were added.
pub struct McpRegistry {
    servers: Vec<Server>,
}

impl Default for McpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl McpRegistry {
    pub fn new() -> Self {
        McpRegistry {
            servers: Vec::new(),
        }
    }

    /// Spawn a server now (program + args, e.g. "npx", &["-y", "…"]).
    /// Spawning eagerly means `tool_defs()` can list what the model can
    /// call before the first turn, and a broken server fails at setup
    /// rather than mid-run.
    pub fn add_server(&mut self, name: &str, program: &str, args: &[&str]) -> Result<(), McpError> {
        // `__` is the namespace separator: a server name containing it
        // would make `mcp__a__b__c` ambiguous at call time.
        if name.is_empty() || name.contains("__") {
            return Err(McpError::Protocol(format!(
                "mcp server name '{name}' is empty or contains the reserved '__'"
            )));
        }
        if self.servers.iter().any(|s| s.name == name) {
            return Err(McpError::Protocol(format!(
                "mcp server '{name}' is already registered"
            )));
        }
        let client = McpClient::spawn(program, args)?;
        self.servers.push(Server {
            name: name.to_string(),
            client,
        });
        Ok(())
    }

    /// Spawned server names, in registration order.
    pub fn names(&self) -> Vec<&str> {
        self.servers.iter().map(|s| s.name.as_str()).collect()
    }

    /// All tools across servers as gateway `ToolDef`s, namespaced so they
    /// can't collide with native tools. Each server is asked for its list
    /// now, so a server that died since spawn fails loudly here instead of
    /// mid-turn.
    pub fn tool_defs(&mut self) -> Result<Vec<ToolDef>, McpError> {
        let mut defs = Vec::new();
        for server in &mut self.servers {
            for tool in server.client.list_tools()? {
                defs.push(ToolDef {
                    name: qualify(&server.name, &tool.name),
                    description: if tool.description.is_empty() {
                        format!("MCP tool '{}' on server '{}'", tool.name, server.name)
                    } else {
                        tool.description
                    },
                    // The server's own inputSchema *is* the parameter
                    // schema — no translation layer to drift.
                    parameters: if tool.input_schema.is_null() {
                        json!({"type": "object", "properties": {}})
                    } else {
                        tool.input_schema
                    },
                });
            }
        }
        Ok(defs)
    }

    /// Route a model tool call to the server that owns its namespace.
    /// Non-`mcp__` names are an error here: native tools are the agent's
    /// job, and quietly answering one from the wrong server would hide a
    /// routing bug.
    pub fn call(&mut self, call: &ToolCall) -> Result<Value, McpError> {
        let (server_name, tool_name) = split(&call.name).ok_or_else(|| {
            McpError::Protocol(format!(
                "'{}' is not an MCP tool name (want {PREFIX}server__tool)",
                call.name
            ))
        })?;
        // Providers send `{}` for no-arg tools and "" when they mean it;
        // both are valid JSON-RPC arguments objects.
        let arguments: Value = if call.arguments.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&call.arguments).map_err(|e| {
                McpError::Protocol(format!("arguments for '{}' are not JSON: {e}", call.name))
            })?
        };
        let server = self
            .servers
            .iter_mut()
            .find(|s| s.name == server_name)
            .ok_or_else(|| McpError::Protocol(format!("no MCP server named '{server_name}'")))?;
        server.client.call_tool(tool_name, arguments)
    }

    /// Load `[[servers]]` entries (`name`, `command`, `args`) from a
    /// `zet.mcp.toml`. A missing file is not an error: no servers
    /// configured is a valid state, and the agent simply gets no extra
    /// tools.
    ///
    /// ```toml
    /// [[servers]]
    /// name = "fs"
    /// command = "npx"
    /// args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    /// ```
    pub fn from_toml(path: &Path) -> Result<Self, McpError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(e) => return Err(McpError::Io(format!("{}: {e}", path.display()))),
        };
        let file: Config = toml::from_str(&text)
            .map_err(|e| McpError::Protocol(format!("{}: {e}", path.display())))?;
        let mut registry = Self::new();
        for server in file.servers {
            let args: Vec<&str> = server.args.iter().map(String::as_str).collect();
            registry.add_server(&server.name, &server.command, &args)?;
        }
        Ok(registry)
    }
}

/// MCP results are content blocks; the model wants text. Text blocks are
/// joined in order, anything else is handed over as compact JSON so
/// nothing is silently dropped, and an `isError` result is labelled as
/// the failure it is.
pub fn render_result(value: &Value) -> String {
    let body = match value.get("content").and_then(|c| c.as_array()) {
        Some(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(|item| match item.get("text").and_then(|t| t.as_str()) {
                    Some(text) => text.to_string(),
                    None => item.to_string(),
                })
                .collect();
            if parts.is_empty() {
                value.to_string()
            } else {
                parts.join("\n")
            }
        }
        None => value.to_string(),
    };
    if value.get("isError").and_then(|v| v.as_bool()) == Some(true) {
        format!("error: {body}")
    } else {
        body
    }
}

fn qualify(server: &str, tool: &str) -> String {
    format!("{PREFIX}{server}__{tool}")
}

/// `mcp__server__tool` -> `(server, tool)`. Splits on the first `__` after
/// the prefix so tool names may themselves contain `__`. `None` for
/// anything not in this namespace.
fn split(name: &str) -> Option<(&str, &str)> {
    let (server, tool) = name.strip_prefix(PREFIX)?.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        None
    } else {
        Some((server, tool))
    }
}

#[derive(Debug, Deserialize, Default)]
struct Config {
    #[serde(default)]
    servers: Vec<ServerSpec>,
}

#[derive(Debug, Deserialize)]
struct ServerSpec {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

/// zet-agent's `mcp__*` dispatch seam (same shape as its `Executor`):
/// implemented here so the agent keeps no MCP dependency of its own.
/// Tool defs still travel separately, via `Agent::with_extra_tools`.
impl zet_agent::McpTools for McpRegistry {
    fn call(&mut self, call: &ToolCall) -> Result<String, String> {
        let result = McpRegistry::call(self, call).map_err(|e| e.to_string())?;
        Ok(render_result(&result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespacing_round_trips() {
        assert_eq!(qualify("fs", "read"), "mcp__fs__read");
        assert_eq!(split("mcp__fs__read"), Some(("fs", "read")));
        // Tool names may contain the separator; the server segment can't.
        assert_eq!(split("mcp__fs__read__file"), Some(("fs", "read__file")));
    }

    #[test]
    fn non_mcp_names_are_rejected() {
        assert_eq!(split("run_shell"), None);
        assert_eq!(split("mcp__fs"), None);
        assert_eq!(split("mcp____read"), None);
        assert_eq!(split("mcp__fs__"), None);
    }

    #[test]
    fn text_content_renders_as_text() {
        let out = render_result(&json!({"content": [{"type": "text", "text": "hi"}]}));
        assert_eq!(out, "hi");
    }

    #[test]
    fn several_blocks_join_and_errors_label() {
        let out = render_result(&json!({
            "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}],
            "isError": true,
        }));
        assert_eq!(out, "error: a\nb");
        // A result with no content blocks is passed through verbatim
        // rather than flattened into an empty string.
        let out = render_result(&json!({"structured": {"n": 1}}));
        assert_eq!(out, r#"{"structured":{"n":1}}"#);
    }

    #[test]
    fn missing_toml_is_an_empty_registry() {
        let missing = std::env::temp_dir().join("zet-mcp-absent-config.toml");
        let _ = std::fs::remove_file(&missing);
        let registry = McpRegistry::from_toml(&missing).unwrap();
        assert!(registry.names().is_empty());
    }

    #[test]
    fn broken_toml_is_an_error() {
        let path = std::env::temp_dir().join(format!("zet-mcp-bad-{}.toml", std::process::id()));
        std::fs::write(&path, "[[servers]\nname = ").unwrap();
        assert!(McpRegistry::from_toml(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
