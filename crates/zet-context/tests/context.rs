use zet_blocks::Block;
use zet_context::{gather, scrub, DEFAULT_BUDGET};

fn block(cmd: &str, output: &str, exit_code: i32) -> Block {
    Block {
        id: 0,
        session: "s".to_string(),
        command: cmd.to_string(),
        output: output.to_string(),
        exit_code,
        cwd: "/home/bandit/zet".to_string(),
        duration_ms: 1,
    }
}

#[test]
fn scrub_redacts_key_patterns() {
    let dirty = concat!(
        "export OPENAI_API_KEY=sk-abc123XYZ4567890qrs\n",
        "token ghp_abcdefghij1234567890 rest\n",
        "aws AKIAIOSFODNN7EXAMPLE here\n",
        "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig\n",
        "password=hunter2 done\n",
        "-----BEGIN RSA PRIVATE KEY-----\n",
    );
    let clean = scrub(dirty);
    for secret in [
        "sk-abc123XYZ4567890qrs",
        "ghp_abcdefghij1234567890",
        "AKIAIOSFODNN7EXAMPLE",
        "eyJhbGciOiJIUzI1NiJ9",
        "hunter2",
        "BEGIN RSA PRIVATE KEY",
    ] {
        assert!(!clean.contains(secret), "leaked {secret}:\n{clean}");
    }
    assert!(clean.contains("[REDACTED]"), "{clean}");
}

#[test]
fn scrub_leaves_normal_text_alone() {
    let text = "commit 79a581e3e566e2877c1c692795e3f4d7b2f560ba\nls -la ~/Desktop\npassword aging is enabled\n";
    let clean = scrub(text);
    assert!(clean.contains("79a581e3e566"), "{clean}");
    assert!(clean.contains("ls -la ~/Desktop"), "{clean}");
    assert!(clean.contains("password aging"), "{clean}");
    assert!(!clean.contains("[REDACTED]"), "{clean}");
}

#[test]
fn gather_caps_budget_recent_first() {
    let blocks: Vec<Block> = (0..20)
        .map(|i| block(&format!("echo {i}"), &"x".repeat(2000), 0))
        .collect();
    let ctx = gather(&blocks, "/home/bandit/zet", None, DEFAULT_BUDGET);
    let text = ctx.render();
    assert!(text.chars().count() <= DEFAULT_BUDGET, "over budget");
    // Newest survives, oldest is dropped.
    assert!(text.contains("echo 19"), "{text}");
    assert!(!text.contains("echo 0"), "{text}");
}

#[test]
fn render_marks_exits_and_cwd() {
    let blocks = vec![
        block("ls ~/Desktop", "a\n", 0),
        block("cat missing", "no such file\n", 1),
    ];
    let ctx = gather(&blocks, "/home/bandit/zet", None, DEFAULT_BUDGET);
    let text = ctx.render();
    assert!(text.contains("cwd: /home/bandit/zet"), "{text}");
    assert!(text.contains("(exit 0)"), "{text}");
    assert!(text.contains("(exit 1)"), "{text}");
    assert!(text.contains("no such file"), "{text}");
}

#[test]
fn render_includes_git_state() {
    let ctx = gather(
        &[block("git status", "ok\n", 0)],
        "/home/bandit/zet",
        Some(zet_context::GitState {
            branch: "master".to_string(),
            dirty: 2,
        }),
        DEFAULT_BUDGET,
    );
    let text = ctx.render();
    assert!(text.contains("git: master"), "{text}");
    assert!(text.contains("dirty: 2"), "{text}");
}

#[test]
fn git_state_none_outside_repo() {
    let dir = std::env::temp_dir().join(format!(
        "zet_ctx_norepo_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(zet_context::git_state(&dir).is_none());
}

#[test]
fn git_state_reads_branch_and_dirt() {
    let dir = std::env::temp_dir().join(format!(
        "zet_ctx_repo_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
            .status
            .success());
    };
    git(&["init", "-q", "-b", "master"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    std::fs::write(dir.join("dirty.txt"), "dirty").unwrap();
    let state = zet_context::git_state(&dir).expect("repo state");
    assert_eq!(state.branch, "master");
    assert_eq!(state.dirty, 1);
}
