// McpRegistry against a stub server: namespacing, routing, config
// loading. No network, no keys.

mod common;

use common::TempDir;
use zet_mcp::{McpError, McpRegistry};
use zet_models::{ToolCall, ToolDef};

fn call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: "c1".to_string(),
        name: name.to_string(),
        arguments: arguments.to_string(),
    }
}

fn names(defs: &[ToolDef]) -> Vec<String> {
    defs.iter().map(|d| d.name.clone()).collect()
}

#[test]
fn tool_defs_are_namespaced_and_call_round_trips() {
    let dir = TempDir::new("defs");
    let stub = dir.stub("alpha");
    let mut registry = McpRegistry::new();
    registry
        .add_server("alpha", "sh", &[stub.to_str().unwrap()])
        .unwrap();

    let defs = registry.tool_defs().unwrap();
    assert_eq!(names(&defs), vec!["mcp__alpha__echo"]);
    assert_eq!(defs[0].description, "Echo back the input text");
    // The server's own inputSchema is the parameter schema.
    assert_eq!(
        defs[0].parameters["properties"]["text"]["type"], "string",
        "{:?}",
        defs[0].parameters
    );
    assert_eq!(defs[0].parameters["required"][0], "text");

    let out = registry
        .call(&call("mcp__alpha__echo", r#"{"text":"ping"}"#))
        .unwrap();
    assert_eq!(out["content"][0]["text"], "alpha: ping");
}

#[test]
fn each_server_gets_its_own_namespace_and_answers() {
    let dir = TempDir::new("route");
    let alpha = dir.stub("alpha");
    let beta = dir.stub("beta");
    let mut registry = McpRegistry::new();
    registry
        .add_server("alpha", "sh", &[alpha.to_str().unwrap()])
        .unwrap();
    registry
        .add_server("beta", "sh", &[beta.to_str().unwrap()])
        .unwrap();
    assert_eq!(registry.names(), vec!["alpha", "beta"]);

    let mut defs: Vec<String> = vec![];
    for def in registry.tool_defs().unwrap() {
        defs.push(def.name);
    }
    assert_eq!(defs, vec!["mcp__alpha__echo", "mcp__beta__echo"]);

    // Same tool, same arguments, two servers: the tag proves the call
    // reached the one named in the call.
    let out = registry
        .call(&call("mcp__beta__echo", r#"{"text":"ping"}"#))
        .unwrap();
    assert_eq!(out["content"][0]["text"], "beta: ping");
}

#[test]
fn native_names_never_reach_a_server() {
    let dir = TempDir::new("native");
    let stub = dir.stub("alpha");
    let mut registry = McpRegistry::new();
    registry
        .add_server("alpha", "sh", &[stub.to_str().unwrap()])
        .unwrap();

    let err = registry
        .call(&call("run_shell", r#"{"command":"ls"}"#))
        .unwrap_err();
    assert!(matches!(err, McpError::Protocol(_)), "{err:?}");
}

#[test]
fn unknown_server_is_an_error() {
    let dir = TempDir::new("unknown");
    let stub = dir.stub("alpha");
    let mut registry = McpRegistry::new();
    registry
        .add_server("alpha", "sh", &[stub.to_str().unwrap()])
        .unwrap();
    let err = registry
        .call(&call("mcp__ghost__echo", r#"{"text":"x"}"#))
        .unwrap_err();
    assert!(matches!(err, McpError::Protocol(_)), "{err:?}");
}

#[test]
fn malformed_arguments_are_a_protocol_error() {
    let dir = TempDir::new("args");
    let stub = dir.stub("alpha");
    let mut registry = McpRegistry::new();
    registry
        .add_server("alpha", "sh", &[stub.to_str().unwrap()])
        .unwrap();
    let err = registry
        .call(&call("mcp__alpha__echo", "not json"))
        .unwrap_err();
    assert!(matches!(err, McpError::Protocol(_)), "{err:?}");
    // Empty arguments mean "no arguments", not broken JSON.
    let out = registry.call(&call("mcp__alpha__echo", "")).unwrap();
    assert_eq!(out["content"][0]["text"], "alpha: ");
}

#[test]
fn server_side_errors_come_back_as_server_errors() {
    let dir = TempDir::new("servererr");
    let stub = dir.stub("alpha");
    let mut registry = McpRegistry::new();
    registry
        .add_server("alpha", "sh", &[stub.to_str().unwrap()])
        .unwrap();
    let err = registry
        .call(&call("mcp__alpha__nope", r#"{}"#))
        .unwrap_err();
    match err {
        McpError::Server { code, message } => {
            assert_eq!(code, -32602);
            assert!(message.contains("unknown tool"), "{message}");
        }
        other => panic!("expected a server error, got {other:?}"),
    }
}

#[test]
fn duplicate_and_reserved_server_names_are_refused() {
    let dir = TempDir::new("names");
    let stub = dir.stub("alpha");
    let mut registry = McpRegistry::new();
    registry
        .add_server("alpha", "sh", &[stub.to_str().unwrap()])
        .unwrap();
    assert!(registry
        .add_server("alpha", "sh", &[stub.to_str().unwrap()])
        .is_err());
    // `__` is the namespace separator: a server name using it would make
    // mcp__a__b__c impossible to route.
    assert!(registry
        .add_server("a__b", "sh", &[stub.to_str().unwrap()])
        .is_err());
    assert!(registry
        .add_server("", "sh", &[stub.to_str().unwrap()])
        .is_err());
    assert_eq!(registry.names(), vec!["alpha"]);
}

#[test]
fn from_toml_spawns_the_configured_servers() {
    let dir = TempDir::new("toml");
    let stub = dir.stub("cfg");
    let config = dir.path().join("zet.mcp.toml");
    std::fs::write(
        &config,
        format!(
            "# zet.mcp.toml\n[[servers]]\nname = \"cfg\"\ncommand = \"sh\"\nargs = [\"{}\"]\n",
            stub.display()
        ),
    )
    .unwrap();

    let mut registry = McpRegistry::from_toml(&config).unwrap();
    assert_eq!(registry.names(), vec!["cfg"]);
    assert_eq!(
        names(&registry.tool_defs().unwrap()),
        vec!["mcp__cfg__echo"]
    );
    let out = registry
        .call(&call("mcp__cfg__echo", r#"{"text":"from-toml"}"#))
        .unwrap();
    assert_eq!(out["content"][0]["text"], "cfg: from-toml");
}

#[test]
fn missing_config_yields_an_empty_registry() {
    let dir = TempDir::new("absent");
    let mut registry = McpRegistry::from_toml(&dir.path().join("nope.toml")).unwrap();
    assert!(registry.names().is_empty());
    assert!(registry.tool_defs().unwrap().is_empty());
}

#[test]
fn a_server_that_will_not_spawn_fails_at_setup() {
    let mut registry = McpRegistry::new();
    let err = registry
        .add_server("ghost", "zet-no-such-mcp-binary", &[])
        .unwrap_err();
    assert!(matches!(err, McpError::Spawn(_)), "{err:?}");
    assert!(registry.names().is_empty());
}
