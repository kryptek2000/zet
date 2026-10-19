// zet#7 acceptance: agent → gateway → MCP tool call → MCP server →
// result → agent, in one run. The gateway is a mock `ChatModel` (no
// network, no key); the MCP server is a stub subprocess speaking
// JSON-RPC over pipes. What is proven here is the loop and the wiring:
// the model is offered the server's tool, the model's call is routed to
// that server, and the server's answer comes back as a Tool-role message
// with the matching tool_call_id — which is what providers require
// before they accept the next turn.
//
// Run: cargo test -p zet-mcp --test e2e

mod common;

use std::cell::RefCell;
use std::collections::VecDeque;

use common::TempDir;
use zet_agent::{Agent, AllowAll};
use zet_mcp::McpRegistry;
use zet_models::{ChatModel, ChatRequest, ChatResponse, Message, Role, ToolCall};
use zet_policy::Tier;

/// Scripted gateway: replies in order and records every request, so the
/// test can see what the model was offered and what it was told.
struct Gateway {
    replies: RefCell<VecDeque<ChatResponse>>,
    seen: RefCell<Vec<ChatRequest>>,
}

impl Gateway {
    fn new(replies: Vec<ChatResponse>) -> Self {
        Gateway {
            replies: RefCell::new(replies.into()),
            seen: RefCell::new(Vec::new()),
        }
    }

    fn final_text(text: &str) -> ChatResponse {
        ChatResponse {
            text: text.to_string(),
            tool_calls: vec![],
        }
    }

    fn tool_call(id: &str, name: &str, arguments: &str) -> ChatResponse {
        ChatResponse {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: arguments.to_string(),
            }],
        }
    }
}

impl ChatModel for Gateway {
    fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, zet_models::ModelError> {
        self.seen.borrow_mut().push(req.clone());
        Ok(self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("gateway script empty"))
    }
}

#[test]
fn agent_calls_an_mcp_tool_and_gets_the_answer_back() {
    let dir = TempDir::new("e2e");
    let stub = dir.stub("test");

    // The registry is built first: its tool list is what the model is
    // offered, and the registry itself then runs the call.
    let mut registry = McpRegistry::new();
    registry
        .add_server("test", "sh", &[stub.to_str().unwrap()])
        .unwrap();
    let defs = registry.tool_defs().unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "mcp__test__echo");

    let gateway = Gateway::new(vec![
        Gateway::tool_call("call_1", "mcp__test__echo", r#"{"text":"ping"}"#),
        Gateway::final_text("the tool said: test: ping"),
        // A server call is T2, so the verify leg runs before the turn ends.
        Gateway::final_text("VERIFIED"),
    ]);
    let mut agent = Agent::new(gateway, AllowAll, false, 8)
        .with_extra_tools(defs)
        .with_mcp_tools(registry);

    assert_eq!(
        agent.run("echo ping through the server").unwrap(),
        "the tool said: test: ping"
    );

    let seen = agent.model.seen.borrow();
    assert_eq!(seen.len(), 3, "step, answer, verify");

    // Turn 1 offered the native tools *and* the server's, namespaced.
    let offered: Vec<String> = seen[0].tools.iter().map(|t| t.name.clone()).collect();
    assert!(
        offered.contains(&"mcp__test__echo".to_string()),
        "{offered:?}"
    );
    assert!(offered.contains(&"run_shell".to_string()), "{offered:?}");

    // Turn 2 carries the assistant turn that made the call…
    let asked: Vec<&ToolCall> = seen[1]
        .messages
        .iter()
        .flat_map(|m| m.tool_calls.iter())
        .collect();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].name, "mcp__test__echo");

    // …and the server's answer as a Tool message with the matching id.
    let results: Vec<&Message> = seen[1]
        .messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .collect();
    assert_eq!(
        results.len(),
        1,
        "one tool result per call: {:?}",
        seen[1].messages
    );
    assert_eq!(results[0].tool_call_id.as_deref(), Some("call_1"));
    assert!(
        results[0].text.contains("test: ping"),
        "server answer never reached the model: {:?}",
        results[0].text
    );

    // The call is in the audit trail like any other step, at the tier it
    // really ran at (the verify leg follows it).
    let entry = agent
        .journal
        .iter()
        .find(|e| e.command.starts_with("mcp__test__echo"))
        .expect("no MCP journal entry");
    assert_eq!(entry.tier, Tier::T2);
    assert!(entry.approved && entry.executed, "{entry:?}");
    assert!(entry.output.contains("test: ping"), "{entry:?}");
}

#[test]
fn a_failing_server_becomes_an_error_result_not_a_dropped_call() {
    let dir = TempDir::new("e2e_err");
    let stub = dir.stub("test");
    let mut registry = McpRegistry::new();
    registry
        .add_server("test", "sh", &[stub.to_str().unwrap()])
        .unwrap();
    let defs = registry.tool_defs().unwrap();

    let gateway = Gateway::new(vec![
        Gateway::tool_call("call_1", "mcp__test__nope", r#"{}"#),
        Gateway::final_text("that tool failed"),
    ]);
    let mut agent = Agent::new(gateway, AllowAll, false, 8)
        .with_extra_tools(defs)
        .with_mcp_tools(registry);
    assert_eq!(
        agent.run("call a missing tool").unwrap(),
        "that tool failed"
    );

    let seen = agent.model.seen.borrow();
    let result = seen[1]
        .messages
        .iter()
        .find(|m| m.role == Role::Tool)
        .expect("the failed call still got an answer");
    assert_eq!(result.tool_call_id.as_deref(), Some("call_1"));
    assert!(result.text.contains("error"), "{:?}", result.text);
    let entry = agent
        .journal
        .iter()
        .find(|e| e.command.starts_with("mcp__test__nope"))
        .expect("the failed call was still audited");
    assert!(!entry.executed);
}
