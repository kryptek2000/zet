use std::process::Command;
use zet_models::keys::Keychain;

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

// Live: piped stdin must store exactly the first line (not block to EOF,
// not swallow following lines). Needs a keychain; CI skips.
#[test]
#[ignore]
fn auth_add_stores_first_stdin_line_only() {
    use std::io::Read as _;
    use std::io::Write;
    use std::process::Stdio;
    let mut child = zet()
        .args(["auth", "add", "compat"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"line1-key\nline2-junk\n")
        .unwrap();
    assert!(child.wait().unwrap().success());
    let mut err = String::new();
    child
        .stderr
        .as_mut()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(err.contains("Paste API key for compat"), "{err}");
    let stored = zet_models::keys::SystemKeychain.get("compat");
    assert_eq!(stored.as_deref(), Some("line1-key"));
    zet_models::keys::SystemKeychain.delete("compat").unwrap();
}
