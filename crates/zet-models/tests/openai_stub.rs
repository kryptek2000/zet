use std::io::{Read, Write};
use std::net::TcpListener;
use zet_models::{ChatModel, ChatRequest, Message, Role, ToolDef};

fn stub_server(listener: TcpListener) -> std::thread::JoinHandle<()> {
    stub_server_with(listener, 200, None, true)
}

fn stub_server_with(
    listener: TcpListener,
    status: u16,
    reason: Option<&'static str>,
    expect_system: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read_exact(&mut byte).is_err() || head.len() > 65536 {
                break;
            }
            head.push(byte[0]);
        }
        let head_text = String::from_utf8_lossy(&head).into_owned();
        let lower = head_text.to_lowercase();
        let len = lower
            .lines()
            .find_map(|l| {
                l.strip_prefix("content-length:")
                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
            })
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).unwrap();

        assert!(
            lower.contains("authorization: bearer sk-test"),
            "missing auth: {head_text}"
        );
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["model"], "stub-model");
        if expect_system {
            assert_eq!(payload["messages"][0]["role"], "system");
        }

        let reply = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "hi there",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "read_file", "arguments": "{\"path\":\"x\"}"}
                    }]
                }
            }]
        });
        let bytes = serde_json::to_vec(&reply).unwrap();
        let reason = reason.unwrap_or("OK");
        write!(
            stream,
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .unwrap();
        stream.write_all(&bytes).unwrap();
    })
}

#[test]
fn openai_compat_roundtrip_against_stub() {
    use zet_models::openai::OpenAICompat;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = stub_server(listener);

    let client = OpenAICompat::new(
        format!("http://{addr}"),
        "sk-test".to_string(),
        "stub-model".to_string(),
    );
    let req = ChatRequest {
        model: "stub-model".to_string(),
        system: Some("sys".to_string()),
        messages: vec![Message {
            role: Role::User,
            text: "hello".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        tools: vec![ToolDef {
            name: "read_file".to_string(),
            description: "read".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }],
        max_tokens: 32,
    };
    let res = client.chat(&req).unwrap();
    assert_eq!(res.text, "hi there");
    assert_eq!(res.tool_calls.len(), 1);
    assert_eq!(res.tool_calls[0].name, "read_file");
    assert_eq!(res.tool_calls[0].arguments, "{\"path\":\"x\"}");
    server.join().unwrap();
}

fn bare_req() -> ChatRequest {
    ChatRequest {
        model: "stub-model".to_string(),
        system: None,
        messages: vec![Message {
            role: Role::User,
            text: "hi".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        tools: vec![],
        max_tokens: 8,
    }
}

#[test]
fn tool_messages_carry_call_id() {
    use zet_models::openai::OpenAICompat;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        let head_text = String::from_utf8_lossy(&head).into_owned().to_lowercase();
        let len = head_text
            .lines()
            .find_map(|l| {
                l.strip_prefix("content-length:")
                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
            })
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Real providers 400 without this (OpenRouter: tool messages must
        // include a non-empty string tool_call_id).
        let tool_msg = payload["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool")
            .expect("no tool message sent");
        assert_eq!(tool_msg["tool_call_id"], "call_9");
        let assistant_msg = payload["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .expect("no assistant message sent");
        assert_eq!(assistant_msg["tool_calls"][0]["id"], "call_9");
        assert_eq!(assistant_msg["tool_calls"][0]["type"], "function");
        let reply =
            serde_json::json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]});
        let bytes = serde_json::to_vec(&reply).unwrap();
        use std::io::Write;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .unwrap();
        stream.write_all(&bytes).unwrap();
    });

    let client = OpenAICompat::new(
        format!("http://{addr}"),
        "sk-test".to_string(),
        "stub-model".to_string(),
    );
    let mut req = bare_req();
    req.messages.push(Message {
        role: Role::Assistant,
        text: String::new(),
        tool_call_id: None,
        tool_calls: vec![zet_models::ToolCall {
            id: "call_9".to_string(),
            name: "run_shell".to_string(),
            arguments: "{}".to_string(),
        }],
    });
    req.messages.push(Message {
        role: Role::Tool,
        text: "output".to_string(),
        tool_call_id: Some("call_9".to_string()),
        tool_calls: vec![],
    });
    let res = client.chat(&req).unwrap();
    assert_eq!(res.text, "ok");
    server.join().unwrap();
}

#[test]
fn provider_errors_map_to_typed_errors() {
    use zet_models::openai::OpenAICompat;
    use zet_models::ModelError;

    for (status, reason, check) in [
        (401u16, "Unauthorized", ModelError::Auth(String::new())),
        (403u16, "Forbidden", ModelError::Auth(String::new())),
        (
            429u16,
            "Too Many Requests",
            ModelError::RateLimited(String::new()),
        ),
        (400u16, "Bad Request", ModelError::Protocol(String::new())),
        (404u16, "Not Found", ModelError::Protocol(String::new())),
        (500u16, "Bad Gateway", ModelError::Server(String::new())),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = stub_server_with(listener, status, Some(reason), false);
        let client = OpenAICompat::new(
            format!("http://{addr}"),
            "sk-test".to_string(),
            "stub-model".to_string(),
        );
        let err = client.chat(&bare_req()).unwrap_err();
        assert_eq!(
            std::mem::discriminant(&err),
            std::mem::discriminant(&check),
            "status {status} gave {err:?}"
        );
        server.join().unwrap();
    }
}
