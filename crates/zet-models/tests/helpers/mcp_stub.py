#!/usr/bin/env python3
"""Minimal fake MCP server over stdio (newline-delimited JSON-RPC).

Speaks just enough of the MCP protocol for zet-models' integration test:
initialize -> tools/list -> tools/call("echo").
"""
import json
import sys


def reply(msg_id, result):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg_id, "result": result}) + "\n")
    sys.stdout.flush()


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        method = msg.get("method")
        msg_id = msg.get("id")
        if method == "initialize":
            reply(msg_id, {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": {"name": "stub", "version": "0.0.0"},
            })
        elif method == "tools/list":
            reply(msg_id, {"tools": [{
                "name": "echo",
                "description": "Echo back the input text",
                "inputSchema": {
                    "type": "object",
                    "properties": {"text": {"type": "string"}},
                },
            }]})
        elif method == "tools/call":
            params = msg.get("params", {})
            name = params.get("name", "")
            if name != "echo":
                sys.stdout.write(json.dumps({
                    "jsonrpc": "2.0", "id": msg_id,
                    "error": {"code": -32602, "message": f"unknown tool: {name}"},
                }) + "\n")
                sys.stdout.flush()
                continue
            text = params.get("arguments", {}).get("text", "")
            reply(msg_id, {"content": [{"type": "text", "text": text}]})
        # Notifications (no id) need no reply.


if __name__ == "__main__":
    main()
