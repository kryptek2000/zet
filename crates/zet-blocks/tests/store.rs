use zet_blocks::{Block, Store};

fn temp_db(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("zet-blocks-{name}-{}.sqlite", std::process::id()))
}

fn sample(session: &str) -> Block {
    Block {
        id: 0,
        session: session.to_string(),
        command: "echo hi".to_string(),
        output: "hi\n".to_string(),
        exit_code: 0,
        cwd: "/tmp".to_string(),
        git_branch: String::new(),
        duration_ms: 3,
    }
}

#[test]
fn append_and_get_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    let id = store.append(&sample("s1")).unwrap();
    let got = store.get(id).unwrap();
    assert_eq!(got.command, "echo hi");
    assert_eq!(got.output, "hi\n");
    assert_eq!(got.exit_code, 0);
    assert_eq!(got.cwd, "/tmp");
}

#[test]
fn list_returns_session_blocks_in_order() {
    let store = Store::open_in_memory().unwrap();
    let mut first = sample("s1");
    first.command = "one".to_string();
    let mut second = sample("s1");
    second.command = "two".to_string();
    second.exit_code = 1;
    let other = sample("s2");
    let id1 = store.append(&first).unwrap();
    store.append(&other).unwrap();
    let id2 = store.append(&second).unwrap();

    let listed = store.list("s1").unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, id1);
    assert_eq!(listed[1].id, id2);
    assert_eq!(listed[1].exit_code, 1);
    assert_eq!(store.list("s2").unwrap().len(), 1);
    assert!(store.list("nope").unwrap().is_empty());
}

#[test]
fn blocks_persist_across_reopen() {
    let path = temp_db("persist");
    let _ = std::fs::remove_file(&path);
    let id = {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        store.append(&sample("s9")).unwrap()
    };
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let got = store.get(id).unwrap();
    assert_eq!(got.session, "s9");
    assert_eq!(got.command, "echo hi");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn replay_renders_ordered_transcript() {
    let store = Store::open_in_memory().unwrap();
    let mut first = sample("s1");
    first.command = "echo hi".to_string();
    first.output = "hi\n".to_string();
    let mut second = sample("s1");
    second.command = "boom".to_string();
    second.output = "kaboom\n".to_string();
    second.exit_code = 1;
    second.duration_ms = 12;
    store.append(&first).unwrap();
    store.append(&second).unwrap();

    assert_eq!(
        store.replay("s1").unwrap(),
        "$ echo hi\nhi\n[exit 0, 3ms]\n$ boom\nkaboom\n[exit 1, 12ms]\n"
    );
    assert_eq!(store.replay("empty").unwrap(), "");
}

#[test]
fn replay_separates_marker_from_unterminated_output() {
    let store = Store::open_in_memory().unwrap();
    let mut block = sample("s1");
    block.command = "printf hi".to_string();
    block.output = "hi".to_string();
    store.append(&block).unwrap();

    assert_eq!(
        store.replay("s1").unwrap(),
        "$ printf hi\nhi\n[exit 0, 3ms]\n"
    );
}

#[test]
fn absurd_duration_errors_instead_of_wrapping() {
    let store = Store::open_in_memory().unwrap();
    let mut block = sample("s1");
    block.duration_ms = u64::MAX;
    assert!(store.append(&block).is_err());
}

#[test]
fn record_live_command_as_block() {
    use zet_pty::Session;

    let mut s = Session::spawn("sh", &["-c", "echo hi; exit 3"], 80, 24).unwrap();
    let mut out = Vec::new();
    let code = s.wait_with_output(&mut out).unwrap();

    let store = Store::open_in_memory().unwrap();
    let id = store
        .append(&Block {
            id: 0,
            session: "live".to_string(),
            command: "echo hi; exit 3".to_string(),
            output: String::from_utf8_lossy(&out).into_owned(),
            exit_code: code as i32,
            cwd: "/tmp".to_string(),
            git_branch: String::new(),
            duration_ms: 0,
        })
        .unwrap();
    let got = store.get(id).unwrap();
    assert!(got.output.contains("hi"), "output was: {:?}", got.output);
    assert_eq!(got.exit_code, 3);
}

#[test]
fn append_scrubs_key_material() {
    let store = Store::open_in_memory().unwrap();
    let raw_cmd = "export K=sk-or-v1-abcdefghij1234567890";
    let raw_out = "token ghp_abcdefghij1234567890 here\nbearer Bearer abcdefgh12345678 end";
    let id = store
        .append(&Block {
            id: 0,
            session: "s".to_string(),
            command: raw_cmd.to_string(),
            output: raw_out.to_string(),
            exit_code: 0,
            cwd: "/tmp".to_string(),
            git_branch: String::new(),
            duration_ms: 0,
        })
        .unwrap();
    let got = store.get(id).unwrap();
    assert!(
        !got.command.contains("sk-or-v1-"),
        "leaked: {:?}",
        got.command
    );
    assert!(!got.output.contains("ghp_"), "leaked: {:?}", got.output);
    assert!(
        !got.output.contains("abcdefgh12345678"),
        "leaked: {:?}",
        got.output
    );
    assert!(
        got.command.contains("[REDACTED]"),
        "marker missing: {:?}",
        got.command
    );
    assert!(
        got.output.contains("[REDACTED]"),
        "marker missing: {:?}",
        got.output
    );
    // Innocent dashes and short tokens survive.
    let id2 = store
        .append(&Block {
            id: 0,
            session: "s".to_string(),
            command: "ls sk-folders run-09999".to_string(),
            output: "ok".to_string(),
            exit_code: 0,
            cwd: "/tmp".to_string(),
            git_branch: String::new(),
            duration_ms: 0,
        })
        .unwrap();
    let got2 = store.get(id2).unwrap();
    assert_eq!(got2.command, "ls sk-folders run-09999");
}

#[test]
fn count_and_recent_commands_are_session_scoped() {
    let store = Store::open_in_memory().unwrap();
    for session in ["a", "b", "a"] {
        store
            .append(&Block {
                id: 0,
                session: session.to_string(),
                command: format!("echo {session}"),
                output: "secret output".to_string(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 3,
            })
            .unwrap();
    }
    assert_eq!(store.count("a").unwrap(), 2);
    assert_eq!(store.count("b").unwrap(), 1);
    assert_eq!(store.count("missing").unwrap(), 0);

    let recent = store.recent_commands("a", 8).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].command, "echo a");
    assert_eq!(recent[1].command, "echo a");
    assert!(recent.iter().all(|block| block.output.is_empty()));
    assert!(store.recent_commands("missing", 8).unwrap().is_empty());
}

#[test]
fn recent_commands_selects_newest_eight_in_display_order() {
    let store = Store::open_in_memory().unwrap();
    for i in 0..12 {
        store
            .append(&Block {
                id: 0,
                session: "s".to_string(),
                command: format!("cmd {i}"),
                output: "output".to_string(),
                exit_code: i,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: i as u64,
            })
            .unwrap();
    }
    let recent = store.recent_commands("s", 8).unwrap();
    assert_eq!(recent.len(), 8);
    assert_eq!(recent[0].command, "cmd 4");
    assert_eq!(recent[7].command, "cmd 11");
    assert!(recent.iter().all(|block| block.output.is_empty()));
}

#[test]
fn recent_commands_empty_history_and_zero_limit() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.recent_commands("s", 8).unwrap().is_empty());
    assert!(store.recent_commands("s", 0).unwrap().is_empty());
    assert_eq!(store.count("s").unwrap(), 0);
}

#[test]
fn append_scrubs_secret_shaped_cwd() {
    let store = Store::open_in_memory().unwrap();
    let mut b = sample("s1");
    b.cwd = "/tmp/sk-abcdefghijklmnopqrstuvwxyz012345".to_string();
    let id = store.append(&b).unwrap();
    let got = store.get(id).unwrap();
    assert_eq!(got.cwd, "/tmp/[REDACTED]");
    assert!(!got.cwd.contains("abcdefghijklmnopqrstuvwxyz"));
}

#[test]
fn legacy_raw_cwd_survives_read() {
    let path =
        std::env::temp_dir().join(format!("zet-blocks-legacy-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "CREATE TABLE blocks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session TEXT NOT NULL,
                command TEXT NOT NULL,
                output TEXT NOT NULL DEFAULT '',
                exit_code INTEGER NOT NULL DEFAULT 0,
                cwd TEXT NOT NULL DEFAULT '',
                git_branch TEXT NOT NULL DEFAULT '',
                duration_ms INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO blocks (session, command, output, exit_code, cwd, git_branch, duration_ms)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                "s1",
                "echo hi",
                "hi",
                0,
                "/tmp/sk-abcdefghijklmnopqrstuvwxyz012345",
                "",
                3
            ],
        )
        .unwrap();
    }
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let got = store.list("s1").unwrap();
    assert_eq!(got.len(), 1);
    // Legacy rows are returned as-is; UI layer is responsible for scrubbing.
    assert!(got[0].cwd.contains("sk-"));
    let _ = std::fs::remove_file(&path);
}
