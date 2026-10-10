use zet_models::{ChatModel, ChatRequest, ChatResponse, Message, ModelError, Role};

struct EchoModel;

impl ChatModel for EchoModel {
    fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, ModelError> {
        let last = req
            .messages
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default();
        Ok(ChatResponse {
            text: format!("echo:{last}"),
            tool_calls: vec![],
        })
    }
}

#[test]
fn fake_model_roundtrips_through_trait() {
    let model = EchoModel;
    let req = ChatRequest {
        model: "test".to_string(),
        system: Some("be brief".to_string()),
        messages: vec![Message {
            role: Role::User,
            text: "hi".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        tools: vec![],
        max_tokens: 64,
    };
    let res = model.chat(&req).unwrap();
    assert_eq!(res.text, "echo:hi");
    assert!(res.tool_calls.is_empty());
}
