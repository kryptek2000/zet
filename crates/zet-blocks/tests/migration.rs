// Schema evolution: databases in the wild predate `git_branch`, and
// `CREATE TABLE IF NOT EXISTS` will not add a column to them. Opening
// one must migrate it, exactly once, without touching its rows.

use zet_blocks::{Block, Store};

fn temp_db(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "zet-blocks-{name}-{}-{nanos}.sqlite",
        std::process::id()
    ))
}

/// The schema before git_branch existed, written with raw SQL so the
/// test does not depend on Store having grown it.
fn write_old_schema(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(
        "CREATE TABLE blocks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            session     TEXT NOT NULL,
            command     TEXT NOT NULL,
            output      TEXT NOT NULL DEFAULT '',
            exit_code   INTEGER NOT NULL DEFAULT 0,
            cwd         TEXT NOT NULL DEFAULT '',
            duration_ms INTEGER NOT NULL DEFAULT 0
        )",
        (),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO blocks (session, command, output, exit_code, cwd, duration_ms)
         VALUES ('old', 'echo legacy', 'legacy\n', 0, '/tmp', 7)",
        (),
    )
    .unwrap();
}

fn block(session: &str, branch: &str) -> Block {
    Block {
        id: 0,
        session: session.to_string(),
        command: "echo hi".to_string(),
        output: "hi\n".to_string(),
        exit_code: 0,
        cwd: "/tmp".to_string(),
        git_branch: branch.to_string(),
        duration_ms: 3,
    }
}

#[test]
fn git_branch_round_trips_through_the_store() {
    let store = Store::open_in_memory().unwrap();
    let id = store.append(&block("s1", "feat/mcp")).unwrap();
    assert_eq!(store.get(id).unwrap().git_branch, "feat/mcp");
    assert_eq!(store.list("s1").unwrap()[0].git_branch, "feat/mcp");
    assert_eq!(store.recent(5).unwrap()[0].git_branch, "feat/mcp");
    // Empty stays empty: not a repo is a normal, common case.
    let id = store.append(&block("s1", "")).unwrap();
    assert_eq!(store.get(id).unwrap().git_branch, "");
}

#[test]
fn old_database_migrates_on_open_and_keeps_its_rows() {
    let path = temp_db("migrate");
    let _ = std::fs::remove_file(&path);
    write_old_schema(&path);

    let store = Store::open(path.to_str().unwrap()).unwrap();
    // Pre-existing row survives, now readable through the new schema.
    let legacy = store.list("old").unwrap();
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0].command, "echo legacy");
    assert_eq!(legacy[0].duration_ms, 7);
    assert_eq!(legacy[0].git_branch, "", "unknown branch reads back empty");

    // New rows write and read the column.
    let id = store.append(&block("new", "main")).unwrap();
    assert_eq!(store.get(id).unwrap().git_branch, "main");
    assert_eq!(store.list("new").unwrap()[0].git_branch, "main");
    // And the replayed transcript is untouched by the migration.
    assert_eq!(
        store.replay("old").unwrap(),
        "$ echo legacy\nlegacy\n[exit 0, 7ms]\n"
    );
    drop(store);

    // Reopening migrates again (idempotent: no error, no duplicate column).
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(store.get(id).unwrap().git_branch, "main");
    drop(store);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn branch_text_goes_through_the_scrub() {
    // Not a secret in practice, but it is arbitrary user text and append
    // is the one place rows are written.
    let store = Store::open_in_memory().unwrap();
    let id = store
        .append(&block("s1", "leak/sk-or-v1-abcdefghij1234567890"))
        .unwrap();
    let branch = store.get(id).unwrap().git_branch;
    assert!(!branch.contains("sk-or-v1-"), "leaked: {branch}");
    assert!(branch.contains("[REDACTED]"), "{branch}");
}
