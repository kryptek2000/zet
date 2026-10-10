use zet_agent::{Agent, DenyNonRead};
use zet_models::keys::{resolve_provider, EnvKeychain, Keychain, SystemKeychain};

// Live end-to-end: agent -> gateway -> real provider. Needs a stored key
// (`zet auth add <provider>`) and network. Spec via ZET_LIVE_SPEC,
// default openrouter gpt-4o-mini (cheap, always exists).
// Run: cargo test -p zet-agent -- --ignored
#[test]
#[ignore]
fn live_agent_turn_through_gateway() {
    let spec =
        std::env::var("ZET_LIVE_SPEC").unwrap_or_else(|_| "openrouter:openai/gpt-4o-mini".into());
    let env = EnvKeychain;
    let sys = SystemKeychain;
    let chains: [&dyn Keychain; 2] = [&env, &sys];
    let provider = resolve_provider(&spec, &chains).unwrap();
    let model = provider.into_client();
    let mut agent = Agent::new(model, DenyNonRead, false, 4);
    let out = agent.run("Reply with exactly the word: live-ok").unwrap();
    assert!(!out.trim().is_empty(), "empty model reply");
    println!("LIVE REPLY: {out}");
    assert!(
        out.contains("live-ok"),
        "model did not follow instructions: {out}"
    );
}
