use zet_agent::{Agent, AllowAll, DenyNonRead};
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

// Live execution: the agent actually runs shell (AllowAll, /tmp only).
// Costs a model call; needs a stored key. Run: -- --ignored
#[test]
#[ignore]
fn live_agent_executes_shell_task() {
    let spec =
        std::env::var("ZET_LIVE_SPEC").unwrap_or_else(|_| "openrouter:openai/gpt-4o-mini".into());
    let env = EnvKeychain;
    let sys = SystemKeychain;
    let chains: [&dyn Keychain; 2] = [&env, &sys];
    let provider = resolve_provider(&spec, &chains).unwrap();
    let model = provider.into_client();
    let proof = "/tmp/zet-live-proof.txt";
    let _ = std::fs::remove_file(proof);
    let mut agent = Agent::new(model, AllowAll, false, 6).with_workdir("/tmp".into());
    let out = agent
        .run(&format!(
            "Use run_shell to write exactly the text agent-was-here into {proof}, then reply DONE"
        ))
        .unwrap();
    let body = std::fs::read_to_string(proof).expect("agent never wrote the file");
    assert_eq!(body.trim(), "agent-was-here");
    assert!(
        agent.journal.iter().any(|e| e.executed),
        "nothing executed: {out}"
    );
    println!("LIVE EXEC JOURNAL: {:?}", agent.journal);
    let _ = std::fs::remove_file(proof);
}
