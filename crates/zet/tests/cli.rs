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
    assert!(text.contains("opencode:"), "{text}");
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

#[test]
fn models_bad_args_fail_loudly_without_network() {
    // Arg validation precedes all IO: safe and deterministic in CI.
    for args in [
        &["models", "--nope"][..],
        &["models", "http://a", "http://b"][..],
    ] {
        let out = zet().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn every_cli_group_dispatches() {
    // The shared list (fallback router + dispatch guard) must agree
    // with the match arms: bogus flags exit 2 through dispatch, never
    // top-level usage, never success, never panic.
    for group in zet_pty::CLI_SUBCOMMANDS {
        // unschedule takes a free id prefix: a miss is exit 1 (like rm),
        // asserted below — not a usage error.
        if *group == "unschedule" {
            continue;
        }
        let out = zet().args([group, "--definitely-bogus"]).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{group}");
    }
    // Bare known group hits the guard hint, not top-level usage.
    let out = zet().args(["auth"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("bad usage for `zet auth`"), "{err}");
    // unschedule miss: well-formed but nothing matches (exit 1, like rm).
    let home = temp_home("unschedule_miss");
    let out = zet()
        .env("HOME", &home)
        .args(["unschedule", "--definitely-bogus"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

fn temp_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zet_home_{}_{}_{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".zet")).unwrap();
    dir
}

fn seed_block(home: &std::path::Path, cmd: &str) {
    let db = home.join(".zet").join("blocks.sqlite");
    let store = zet_blocks::Store::open(db.to_str().unwrap()).unwrap();
    store
        .append(&zet_blocks::Block {
            id: 0,
            session: "s".to_string(),
            command: cmd.to_string(),
            output: "out".to_string(),
            exit_code: 0,
            cwd: "/tmp".to_string(),
            duration_ms: 1,
        })
        .unwrap();
}

fn seed_turn(home: &std::path::Path) {
    zet_agent::append_journal(
        &home.join(".zet").join("journal.jsonl"),
        &[zet_agent::JournalEntry {
            command: "echo agent-step".to_string(),
            tier: zet_policy::Tier::T1,
            approval: zet_policy::Approval::Ask,
            approved: true,
            executed: true,
            exit_code: Some(0),
            output: "out".to_string(),
            checkpoint: None,
        }],
    )
    .unwrap();
}

#[test]
fn journal_unifies_turns_and_recent_commands() {
    let home = temp_home("unified");
    seed_block(&home, "ls ~/Desktop");
    seed_turn(&home);
    let out = zet()
        .env("HOME", &home)
        .args(["journal", "--limit", "5"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("echo agent-step"), "{text}");
    assert!(text.contains("recent commands:"), "{text}");
    assert!(text.contains("ls ~/Desktop"), "{text}");
}

#[test]
fn journal_without_turns_still_shows_commands() {
    let home = temp_home("commands_only");
    seed_block(&home, "mkdir new_folder");
    let out = zet().env("HOME", &home).args(["journal"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("no agent turns recorded"), "{text}");
    assert!(text.contains("mkdir new_folder"), "{text}");
}

#[test]
fn undo_without_checkpoint_says_so() {
    let home = temp_home("undo_empty");
    let out = zet().env("HOME", &home).args(["undo"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("nothing to undo"), "{text}");
}

#[test]
fn undo_restores_repo_from_journal_checkpoint() {
    let home = temp_home("undo_repo");
    let repo = home.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let st = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(st.status.success(), "{args:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(repo.join("keep.txt"), "keep").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let head = String::from_utf8_lossy(
        &std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    // The "agent step": junk appears after the checkpoint.
    zet_agent::append_journal(
        &home.join(".zet").join("journal.jsonl"),
        &[zet_agent::JournalEntry {
            command: "echo junk > junk.txt".to_string(),
            tier: zet_policy::Tier::T1,
            approval: zet_policy::Approval::Ask,
            approved: true,
            executed: true,
            exit_code: Some(0),
            output: "out".to_string(),
            checkpoint: Some(zet_agent::Checkpoint { head, stash: None }),
        }],
    )
    .unwrap();
    std::fs::write(repo.join("junk.txt"), "junk").unwrap();
    let out = zet()
        .env("HOME", &home)
        .current_dir(&repo)
        .args(["undo"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    assert!(!repo.join("junk.txt").exists(), "junk survived undo");
    assert!(repo.join("keep.txt").exists(), "keep lost in undo");
}

#[test]
fn undo_refuses_when_head_moved_on() {
    let home = temp_home("undo_moved");
    let repo = home.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let st = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(st.status.success(), "{args:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(repo.join("keep.txt"), "keep").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let head = String::from_utf8_lossy(
        &std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    zet_agent::append_journal(
        &home.join(".zet").join("journal.jsonl"),
        &[zet_agent::JournalEntry {
            command: "echo junk > junk.txt".to_string(),
            tier: zet_policy::Tier::T1,
            approval: zet_policy::Approval::Ask,
            approved: true,
            executed: true,
            exit_code: Some(0),
            output: "out".to_string(),
            checkpoint: Some(zet_agent::Checkpoint { head, stash: None }),
        }],
    )
    .unwrap();
    // Life went on: a commit landed after the checkpoint. Undo must
    // refuse rather than revert it.
    std::fs::write(repo.join("later.txt"), "later").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "later"]);
    std::fs::write(repo.join("junk.txt"), "junk").unwrap();
    let out = zet()
        .env("HOME", &home)
        .current_dir(&repo)
        .args(["undo"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "undo reverted a later commit");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("moved on"), "{err}");
    assert!(repo.join("later.txt").exists(), "later commit lost");
    assert!(
        repo.join("junk.txt").exists(),
        "junk should survive refusal"
    );
}

#[test]
fn run_rejects_flags_and_empty_goal() {
    let out = zet().args(["run", "--fast"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = zet().args(["run"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn run_background_needs_goal() {
    let out = zet().args(["run", "--background"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn run_background_rejects_flags_after() {
    let out = zet()
        .args(["run", "--background", "--foo"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn runs_empty_reports_none() {
    let home = temp_home("runs_empty");
    let out = zet().env("HOME", &home).args(["runs"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("no background runs"), "{text}");
}

#[test]
fn runs_rejects_flags() {
    let out = zet().args(["runs", "--all"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn run_bg_alias_delegates() {
    // --bg (spec §5) behaves like --background: needs a goal, not flags.
    let out = zet().args(["run", "--bg"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = zet().args(["run", "-b", "--foo"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

fn seed_recipe(home: &std::path::Path, name: &str, body: &str) {
    let dir = home.join(".zet").join("recipes");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
}

#[test]
fn run_recipe_missing_arg_fails_loud() {
    let home = temp_home("recipe_arg");
    seed_recipe(&home, "greet", "Say hello to {{name}}");
    let out = zet()
        .env("HOME", &home)
        .args(["run", "recipe:greet"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--arg name="), "{err}");
}

#[test]
fn run_unknown_recipe_lists_available() {
    let home = temp_home("recipe_unknown");
    seed_recipe(&home, "greet", "Say hello");
    let out = zet()
        .env("HOME", &home)
        .args(["run", "recipe:nope"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("greet"), "{err}");
}

#[test]
fn usage_reports_seeded_log() {
    let home = temp_home("usage");
    let log = home.join(".zet").join("usage.jsonl");
    std::fs::write(
        &log,
        "{\"ts\":1,\"label\":\"opencode:spark\",\"in_chars\":400,\"out_chars\":400,\"in_est\":100,\"out_est\":100,\"ms\":50}\n\
         {\"ts\":2,\"label\":\"local:llama\",\"in_chars\":80,\"out_chars\":40,\"in_est\":20,\"out_est\":10,\"ms\":150}\n\
         garbage-line\n",
    )
    .unwrap();
    let out = zet().env("HOME", &home).args(["usage"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("2 calls"), "{text}");
    assert!(text.contains("opencode:spark"), "{text}");
    assert!(text.contains("local:llama"), "{text}");
    assert!(text.contains("estimated"), "{text}");
}

#[test]
fn usage_empty_reports_none() {
    let home = temp_home("usage_empty");
    let out = zet().env("HOME", &home).args(["usage"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("no usage recorded"), "{text}");
}

#[test]
fn run_recipe_traversal_rejected() {
    let home = temp_home("recipe_trav");
    std::fs::write(home.join("secret.md"), "Say pwned").unwrap();
    for evil in ["recipe:../secret", "recipe:/etc/hostname", "recipe:.."] {
        let out = zet()
            .env("HOME", &home)
            .args(["run", evil])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{evil}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("bad recipe name"), "{evil}: {err}");
    }
}

#[test]
fn run_recipe_unterminated_placeholder_fails() {
    let home = temp_home("recipe_unterm");
    seed_recipe(&home, "broken", "Say {{oops");
    let out = zet()
        .env("HOME", &home)
        .args(["run", "recipe:broken"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unterminated"), "{err}");
}

#[test]
fn run_every_creates_schedule() {
    let home = temp_home("sched_create");
    let out = zet()
        .env("HOME", &home)
        .env("ZET_INLINE_SPEC", "local:http://127.0.0.1:9/dead")
        .args(["run", "--every", "1h", "say hi"])
        .output()
        .unwrap();
    // No model here: delegation must still record the schedule first.
    let sched =
        std::fs::read_to_string(home.join(".zet").join("schedules.jsonl")).unwrap_or_default();
    assert!(sched.contains("say hi"), "{sched}");
    assert!(sched.contains("\"every_secs\":3600"), "{sched}");
    let _ = out;
}

#[test]
fn run_every_bad_value_fails_loud() {
    for args in [&["run", "--every"][..], &["run", "--every", "soon"][..]] {
        let out = zet().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn schedules_empty_reports_none() {
    let home = temp_home("sched_empty");
    let out = zet()
        .env("HOME", &home)
        .args(["schedules"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("no schedules"), "{text}");
}

#[test]
fn unschedule_removes_by_prefix() {
    let home = temp_home("sched_rm");
    std::fs::create_dir_all(home.join(".zet")).unwrap();
    std::fs::write(
        home.join(".zet").join("schedules.jsonl"),
        "{\"id\":\"abc123\",\"goal\":\"say hi\",\"every_secs\":3600,\"last_run\":0}\n",
    )
    .unwrap();
    let out = zet()
        .env("HOME", &home)
        .args(["unschedule", "abc"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let left = std::fs::read_to_string(home.join(".zet").join("schedules.jsonl")).unwrap();
    assert!(!left.contains("abc123"), "{left}");
}

#[test]
fn daemon_fires_due_schedule() {
    let home = temp_home("daemon_fire");
    std::fs::create_dir_all(home.join(".zet")).unwrap();
    std::fs::write(
        home.join(".zet").join("schedules.jsonl"),
        "{\"id\":\"sch-due-1\",\"goal\":\"say scheduled\",\"every_secs\":60,\"last_run\":0}\n",
    )
    .unwrap();
    // Daemon runs forever; timeout kills it. Worker is detached and fails
    // fast with no model here (no keys, localhost refused) — deterministic.
    let _ = std::process::Command::new("timeout")
        .args(["-s", "KILL", "4"])
        .arg(env!("CARGO_BIN_EXE_zet"))
        .arg("daemon")
        .env("HOME", &home)
        // Hermetic: dead local endpoint fails fast (refused) instead of
        // autodetecting a real server and burning minutes on a model call.
        .env("ZET_INLINE_SPEC", "local:http://127.0.0.1:9/dead")
        .output();
    let runs = home.join(".zet").join("runs");
    let mut done = false;
    for _ in 0..40 {
        if let Ok(entries) = std::fs::read_dir(&runs) {
            for e in entries.flatten() {
                let status = e.path().join("status");
                if std::fs::read_to_string(&status)
                    .unwrap_or_default()
                    .contains("done")
                {
                    done = true;
                    break;
                }
            }
        }
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert!(done, "daemon never delegated the due schedule");
}

#[test]
fn run_quoted_recipe_hints_unquoted_form() {
    let home = temp_home("recipe_hint");
    seed_recipe(&home, "greet", "Say hi {{name}}");
    // One quoted word (beginner trap): fail loud AND show the fix.
    let out = zet()
        .env("HOME", &home)
        .args(["run", "recipe:greet --arg name=world"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown recipe"), "{err}");
    assert!(err.contains("no quotes"), "{err}");
}
