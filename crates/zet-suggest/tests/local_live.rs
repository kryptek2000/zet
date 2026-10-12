use zet_models::openai::OpenAICompat;
use zet_models::{ChatModel, ChatRequest, Message, Role};
use zet_suggest::ModelSuggester;

// Live local inference: needs `ollama serve` + a pulled model
// (setup: ollama pull qwen3:1.7b). No key, no network beyond
// localhost. Ignored by default; run with -- --ignored.
fn local_client() -> OpenAICompat {
    // llama3.2:1b answers in seconds on CPU; qwen3:1.7b reasons
    // (~100s+) — override via ZET_LOCAL_SPEC for quality runs.
    let spec = std::env::var("ZET_LOCAL_SPEC").unwrap_or_else(|_| "local:llama3.2:1b".into());
    let none: &[&dyn zet_models::keys::Keychain] = &[];
    let provider =
        zet_models::keys::resolve_provider(&spec, none).expect("local spec must parse keyless");
    match provider {
        zet_models::Provider::Local { endpoint, model } => {
            OpenAICompat::new(endpoint, String::new(), model)
        }
        other => panic!("expected Local, got {other:?}"),
    }
}

#[test]
#[ignore]
fn live_local_chat_answers() {
    let client = local_client();
    let res = client
        .chat(&ChatRequest {
            model: String::new(),
            system: None,
            messages: vec![Message {
                role: Role::User,
                text: "Reply with exactly: local-ok".to_string(),
                tool_call_id: None,
                tool_calls: vec![],
            }],
            tools: vec![],
            max_tokens: 16,
        })
        .expect("ollama up?");
    // Wiring proof only: 1b models don't follow exact-output
    // instructions reliably. Non-empty means spec→client→server→
    // model→parse all work.
    println!("LOCAL REPLY: {:?}", res.text);
    assert!(!res.text.trim().is_empty(), "empty reply");
}

#[test]
#[ignore]
fn live_local_nl_suggests_command() {
    let client = local_client();
    let s = ModelSuggester::new(client);
    let cmd = s
        .suggest_nl("list files in the current directory")
        .unwrap()
        .expect("no suggestion");
    println!("LOCAL NL: {cmd}");
    assert!(!cmd.is_empty());
    assert!(!cmd.contains('\n'), "must be single-line: {cmd:?}");
}
