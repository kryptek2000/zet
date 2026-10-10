use std::process::Command;

fn zet() -> Command {
    let bin = env!("CARGO_BIN_EXE_zet");
    Command::new(bin)
}

#[test]
fn auth_show_lists_providers_without_crashing() {
    // Read-only: safe everywhere, including keyring-less CI (shows missing).
    let out = zet().args(["auth", "show"]).output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("openrouter"), "{text}");
}

#[test]
fn auth_bad_usage_fails_loudly() {
    for args in [&["auth"][..], &["auth", "add"][..], &["bogus"][..]] {
        let out = zet().args(args).output().unwrap();
        assert!(!out.status.success(), "{args:?}");
    }
}
