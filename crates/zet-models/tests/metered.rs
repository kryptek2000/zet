use zet_models::{ChatModel, ChatRequest, ChatResponse, Message, Role};

struct Stub;

impl ChatModel for Stub {
    fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, zet_models::ModelError> {
        let n: usize = req.messages.iter().map(|m| m.text.len()).sum();
        Ok(ChatResponse {
            text: "x".repeat(n),
            tool_calls: vec![],
        })
    }
}

fn req(text: &str) -> ChatRequest {
    ChatRequest {
        model: String::new(),
        system: Some("sys".to_string()),
        messages: vec![Message {
            role: Role::User,
            text: text.to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        tools: vec![],
        max_tokens: 16,
    }
}

#[test]
fn metered_logs_estimated_tokens() {
    let dir = std::env::temp_dir().join(format!(
        "zet_meter_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("usage.jsonl");
    let m = zet_models::Metered::new(Stub, "opencode:spark", log.clone());
    let res = m.chat(&req(&"a".repeat(400))).unwrap();
    assert_eq!(res.text.len(), 400);
    let lines = std::fs::read_to_string(&log).unwrap();
    assert!(lines.contains("opencode:spark"), "{lines}");
    // 400 in + 3 sys ≈ 100 tokens estimated (chars/4).
    let v: serde_json::Value = serde_json::from_str(lines.trim()).unwrap();
    assert!(v["in_est"].as_u64().unwrap() >= 90, "{lines}");
    assert!(v["out_est"].as_u64().unwrap() >= 90, "{lines}");
    assert!(v["ms"].as_u64().is_some(), "{lines}");
}

#[test]
fn metered_rotates_past_1mb() {
    let dir = std::env::temp_dir().join(format!(
        "zet_meter_rot_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("usage.jsonl");
    // Seed past the cap with 1KB lines (oldest first).
    let mut seed = String::new();
    for i in 0..1200 {
        seed.push_str(&format!("{{\"seed\":{i},{}}}\n", "x".repeat(1000)));
    }
    std::fs::write(&log, &seed).unwrap();
    assert!(std::fs::metadata(&log).unwrap().len() > 1 << 20);
    let m = zet_models::Metered::new(Stub, "fresh", log.clone());
    m.chat(&req("hi")).unwrap();
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        std::fs::metadata(&log).unwrap().len() <= 1 << 20,
        "still oversized"
    );
    // Fresh line survived, oldest seeds did not.
    assert!(text.contains("\"fresh\""), "new line lost");
    assert!(!text.contains("\"seed\":0"), "oldest seed survived");
    assert!(text.lines().count() <= 4001, "tail too long");
}
