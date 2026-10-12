use std::io::{Read, Write};
use std::net::TcpListener;
use zet_models::go::GoClient;
use zet_models::{ChatModel, ChatRequest, Message, Role};

fn read_request(stream: &mut std::net::TcpStream) -> (String, Vec<u8>) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read_exact(&mut byte).is_err() || head.len() > 65536 {
            break;
        }
        head.push(byte[0]);
    }
    let head_text = String::from_utf8_lossy(&head).into_owned();
    let len = head_text
        .to_lowercase()
        .lines()
        .find_map(|l| {
            l.strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).unwrap_or(());
    (head_text, body)
}

fn reply(stream: &mut std::net::TcpStream, status: u16, body: &str) {
    let text = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(text.as_bytes()).unwrap();
}

fn assert_session(head: &str) {
    let lower = head.to_lowercase();
    for h in [
        "x-opencode-session: ses_",
        "x-opencode-session-id: ses_",
        "x-session-affinity: ses_",
        "x-session-id: ses_",
    ] {
        assert!(lower.contains(h), "missing {h} in:\n{head}");
    }
}

fn req() -> ChatRequest {
    ChatRequest {
        model: String::new(),
        system: None,
        messages: vec![Message {
            role: Role::User,
            text: "hi".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        tools: vec![],
        max_tokens: 16,
    }
}

#[test]
fn chat_success_returns_text() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (head, _) = read_request(&mut stream);
        assert!(head.contains("POST /chat/completions"), "{head}");
        assert_session(&head);
        reply(
            &mut stream,
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":"hey"}}]}"#,
        );
    });
    let c = GoClient::new(
        format!("http://127.0.0.1:{port}"),
        "test-key".to_string(),
        "chat-ok-model".to_string(),
    );
    let res = c.chat(&req()).unwrap();
    assert_eq!(res.text, "hey");
    h.join().unwrap();
}

#[test]
fn protocol_unsupported_falls_back_to_responses() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        // Leg 1: chat → ModelProtocolUnsupported.
        let (mut stream, _) = listener.accept().unwrap();
        let (head, _) = read_request(&mut stream);
        assert!(head.contains("POST /chat/completions"), "{head}");
        assert_session(&head);
        reply(
            &mut stream,
            400,
            r#"{"type":"error","error":{"type":"ModelProtocolUnsupported","message":"Model does not support this protocol."}}"#,
        );
        // Leg 2: responses → text joins.
        let (mut stream, _) = listener.accept().unwrap();
        let (head, body) = read_request(&mut stream);
        assert!(head.contains("POST /responses"), "{head}");
        assert_session(&head);
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["model"], "proto-fallback-model");
        // One-liner leg: minimal thinking, floored budget.
        assert_eq!(payload["reasoning"]["effort"], "minimal");
        assert!(
            payload["max_output_tokens"].as_u64().unwrap_or(0) >= 512,
            "{payload}"
        );
        reply(
            &mut stream,
            200,
            r#"{"output":[{"content":[{"text":"hel"},{"text":"lo"}]}]}"#,
        );
    });
    let c = GoClient::new(
        format!("http://127.0.0.1:{port}"),
        "test-key".to_string(),
        "proto-fallback-model".to_string(),
    );
    let res = c.chat(&req()).unwrap();
    assert_eq!(res.text, "hello");
    h.join().unwrap();
}

#[test]
fn session_ids_look_right() {
    let a = zet_models::go::new_session_id();
    let b = zet_models::go::new_session_id();
    for s in [&a, &b] {
        assert!(s.starts_with("ses_"), "{s}");
        assert_eq!(s.len(), 68, "{s}");
        assert!(s[4..].chars().all(|c| c.is_ascii_hexdigit()), "{s}");
    }
    assert_ne!(a, b);
}

fn agent_req() -> ChatRequest {
    ChatRequest {
        model: String::new(),
        system: Some("agent".to_string()),
        messages: vec![Message {
            role: Role::User,
            text: "do it".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        tools: vec![zet_models::ToolDef {
            name: "run_shell".to_string(),
            description: "run".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }],
        max_tokens: 16,
    }
}

#[test]
fn chat_leg_carries_tools_and_parses_calls() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (head, body) = read_request(&mut stream);
        assert!(head.contains("POST /chat/completions"), "{head}");
        assert_session(&head);
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["tools"][0]["function"]["name"], "run_shell");
        reply(
            &mut stream,
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":"","tool_calls":[{"id":"c1","type":"function","function":{"name":"run_shell","arguments":"{\"command\":\"ls\"}"}}]}}]}"#,
        );
    });
    let c = GoClient::new(
        format!("http://127.0.0.1:{port}"),
        "test-key".to_string(),
        "chat-tools-model".to_string(),
    );
    let res = c.chat(&agent_req()).unwrap();
    assert_eq!(res.tool_calls.len(), 1);
    assert_eq!(res.tool_calls[0].name, "run_shell");
    assert!(res.tool_calls[0].arguments.contains("ls"));
    h.join().unwrap();
}

#[test]
fn responses_leg_refuses_tools_loudly() {
    // No stub: must fail before any POST (agent turns never run blind).
    let c = GoClient::new(
        "http://127.0.0.1:9".to_string(),
        "test-key".to_string(),
        "responses-tools-model".to_string(),
    );
    // Force the responses leg by pre-caching the protocol... cache is
    // process-global keyed by model: first trigger fallback via stub.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (_, _) = read_request(&mut stream);
        reply(
            &mut stream,
            400,
            r#"{"error":{"type":"ModelProtocolUnsupported","message":"nope"}}"#,
        );
    });
    let c2 = GoClient::new(
        format!("http://127.0.0.1:{port}"),
        "test-key".to_string(),
        "responses-tools-model".to_string(),
    );
    // Chat 400 with unsupported type: falls to responses leg, which
    // refuses tools loudly (no second connection needed).
    let err = c2.chat(&agent_req()).unwrap_err().to_string();
    assert!(err.contains("no function calling"), "{err}");
    h.join().unwrap();
    let _ = c;
}
