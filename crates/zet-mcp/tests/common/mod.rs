//! Test scaffolding shared by the zet-mcp test binaries: a stub MCP
//! server written to a temp dir at run time. Nothing here touches the
//! network or a keychain — the stub speaks newline-delimited JSON-RPC
//! over pipes, like every real MCP server.

use std::path::{Path, PathBuf};

/// Stub MCP server: `initialize`, `tools/list` (one `echo` tool),
/// `tools/call`. `echo` replies with its argument prefixed by `tag`, so a
/// test can tell *which* server answered a routed call.
///
/// Started as `sh <path>`, so no executable bit is needed.
fn script(tag: &str) -> String {
    format!(
        r#"#!/bin/sh
STUB_TAG='{tag}'
rpc_id() {{
  printf '%s' "$1" | sed -n 's/.*"id"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p'
}}
while IFS= read -r line; do
  id=$(rpc_id "$line")
  case "$line" in
    *'"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":"2024-11-05","capabilities":{{}},"serverInfo":{{"name":"stub","version":"0"}}}}}}\n' "$id"
      ;;
    *'"tools/list"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"tools":[{{"name":"echo","description":"Echo back the input text","inputSchema":{{"type":"object","properties":{{"text":{{"type":"string"}}}},"required":["text"]}}}}]}}}}\n' "$id"
      ;;
    *'"tools/call"'*)
      name=$(printf '%s' "$line" | sed -n 's/.*"name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
      if [ "$name" != "echo" ]; then
        printf '{{"jsonrpc":"2.0","id":%s,"error":{{"code":-32602,"message":"unknown tool: %s"}}}}\n' "$id" "$name"
      else
        text=$(printf '%s' "$line" | sed -n 's/.*"text"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
        printf '{{"jsonrpc":"2.0","id":%s,"result":{{"content":[{{"type":"text","text":"%s: %s"}}]}}}}\n' "$id" "$STUB_TAG" "$text"
      fi
      ;;
  esac
done
"#
    )
}

/// A temp directory that deletes itself when the test ends.
pub struct TempDir(PathBuf);

impl TempDir {
    // Each test binary compiles this module separately and uses a
    // different slice of it; unused helpers here are not dead weight in
    // either.
    #[allow(dead_code)]
    pub fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("zet-mcp-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        TempDir(dir)
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Write a stub server tagged `tag`; returns its path.
    #[allow(dead_code)]
    pub fn stub(&self, tag: &str) -> PathBuf {
        let path = self.0.join(format!("stub-{tag}.sh"));
        std::fs::write(&path, script(tag)).expect("write stub");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
