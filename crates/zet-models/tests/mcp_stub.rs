use zet_models::mcp::McpClient;

fn stub_path() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    format!("{dir}/tests/helpers/mcp_stub.py")
}

#[test]
fn mcp_list_and_call_against_stub() {
    let mut client = McpClient::spawn("python3", &[stub_path().as_str()]).unwrap();
    let tools = client.list_tools().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");

    let out = client
        .call_tool("echo", serde_json::json!({"text": "hi"}))
        .unwrap();
    assert_eq!(out["content"][0]["text"], "hi");
}

#[test]
fn mcp_unknown_tool_is_a_server_error() {
    let mut client = McpClient::spawn("python3", &[stub_path().as_str()]).unwrap();
    let err = client.call_tool("nope", serde_json::json!({})).unwrap_err();
    assert!(
        matches!(err, zet_models::McpError::Server { .. }),
        "{err:?}"
    );
}
