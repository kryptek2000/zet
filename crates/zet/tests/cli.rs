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

#[test]
fn blocks_missing_flag_values_fail_loudly() {
    for args in [
        &["blocks", "--db"][..],
        &["blocks", "--limit"][..],
        &["blocks", "--limit", "abc"][..],
        &["blocks", "--session"][..],
        &["blocks", "--nope"][..],
    ] {
        let out = zet().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn blocks_lists_seeded_db() {
    let dir = std::env::temp_dir().join(format!(
        "zet_blocks_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("b.sqlite");
    let store = zet_blocks::Store::open(db.to_str().unwrap()).unwrap();
    for (i, cmd) in ["echo alpha", "echo beta"].iter().enumerate() {
        store
            .append(&zet_blocks::Block {
                id: 0,
                session: "s".to_string(),
                command: cmd.to_string(),
                output: format!("out{i}"),
                exit_code: i as i32,
                cwd: "/tmp".to_string(),
                duration_ms: 1,
            })
            .unwrap();
    }
    let out = zet()
        .args(["blocks", "--db", db.to_str().unwrap(), "--limit", "10"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("echo alpha"), "{text}");
    assert!(text.contains("echo beta"), "{text}");
    assert!(text.contains("[exit 1]"), "{text}");
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

#[test]
fn inline_show_is_read_only_and_names_a_source() {
    // No fs writes: safe against the real ~/.zet. ZET_INLINE_SPEC may or
    // may not be set in this env — either way a source is named.
    let out = zet().arg("inline").output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("(env)")
            || text.contains("`zet inline`")
            || text.contains("(autodetected local)")
            || text.contains("unset"),
        "{text}"
    );
}

#[test]
fn inline_bad_spec_fails_loudly_without_writing() {
    // No colon: rejected before any fs touch.
    let out = zet().args(["inline", "gpt-4o-mini"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{:?}", out.status);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("bad spec"), "{err}");
}
