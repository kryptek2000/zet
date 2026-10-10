// MCP client over child-process stdio (newline-delimited JSON-RPC).
// Speaks the client side of the handshake: initialize, notifications/
// initialized, tools/list, tools/call. Servers are spawned as subprocesses
// (npx, uvx, or a local binary) and die with the client on drop.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug)]
pub enum McpError {
    Spawn(String),
    Io(String),
    Protocol(String),
    Server { code: i64, message: String },
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::Spawn(e) => write!(f, "spawn: {e}"),
            McpError::Io(e) => write!(f, "io: {e}"),
            McpError::Protocol(e) => write!(f, "protocol: {e}"),
            McpError::Server { code, message } => {
                write!(f, "server error {code}: {message}")
            }
        }
    }
}

impl std::error::Error for McpError {}

pub struct McpClient {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    _child: Child,
}

impl McpClient {
    pub fn spawn(program: &str, args: &[&str]) -> Result<Self, McpError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| McpError::Spawn(format!("{program}: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Spawn("server did not expose stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Spawn("server did not expose stdout".to_string()))?;
        let mut client = McpClient {
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            _child: child,
        };
        client.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "zet", "version": env!("CARGO_PKG_VERSION")},
            }),
        )?;
        client.notify("notifications/initialized", json!({}))?;
        Ok(client)
    }

    pub fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let result = self.request("tools/list", json!({}))?;
        let tools = result
            .get("tools")
            .and_then(|t| t.as_array())
            .ok_or_else(|| McpError::Protocol("tools/list missing tools[]".to_string()))?;
        tools
            .iter()
            .map(|t| {
                Ok(McpTool {
                    name: t
                        .get("name")
                        .and_then(|n| n.as_str())
                        .ok_or_else(|| McpError::Protocol("tool missing name".to_string()))?
                        .to_string(),
                    description: t
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    input_schema: t.get("inputSchema").cloned().unwrap_or(Value::Null),
                })
            })
            .collect()
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, McpError> {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let line = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .map_err(|e| McpError::Protocol(e.to_string()))?;
        writeln!(self.stdin, "{line}").map_err(|e| McpError::Io(e.to_string()))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;
        let line = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .map_err(|e| McpError::Protocol(e.to_string()))?;
        writeln!(self.stdin, "{line}").map_err(|e| McpError::Io(e.to_string()))?;
        self.stdin
            .flush()
            .map_err(|e| McpError::Io(e.to_string()))?;

        let mut reply = String::new();
        loop {
            reply.clear();
            let n = self
                .stdout
                .read_line(&mut reply)
                .map_err(|e| McpError::Io(e.to_string()))?;
            if n == 0 {
                return Err(McpError::Io("server closed stdout".to_string()));
            }
            let msg: Value = serde_json::from_str(reply.trim())
                .map_err(|e| McpError::Protocol(e.to_string()))?;
            // Skip notifications / mismatched ids; servers may interleave them.
            if msg.get("id").and_then(|v| v.as_u64()) != Some(id) {
                continue;
            }
            if let Some(error) = msg.get("error") {
                return Err(McpError::Server {
                    code: error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1),
                    message: error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                });
            }
            return msg
                .get("result")
                .cloned()
                .ok_or_else(|| McpError::Protocol("response missing result".to_string()));
        }
    }
}
