use zet_blocks::{Block, Store};

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
fn mission_create_persist_and_list() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .create_mission("Fix build", "track build commands")
        .unwrap();
    assert!(id > 0);
    let got = store.get_mission(id).unwrap().unwrap();
    assert_eq!(got.title, "Fix build");
    assert_eq!(got.description, "track build commands");
    let list = store.list_missions().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
}

#[test]
fn mission_link_unlink_and_availability() {
    let store = Store::open_in_memory().unwrap();
    let mission_id = store.create_mission("M", "").unwrap();
    let block_id = store.append(&sample("s1")).unwrap();

    assert!(store
        .link_command(mission_id, block_id, "s1", "user")
        .unwrap());
    assert!(!store
        .link_command(mission_id, block_id, "s1", "user")
        .unwrap());

    let cmds = store.mission_commands(mission_id).unwrap();
    assert_eq!(cmds.len(), 1);
    assert!(cmds[0].available);

    assert!(store
        .mission_command_block(block_id, "s1")
        .unwrap()
        .is_some());

    assert!(store.unlink_command(mission_id, block_id, "s1").unwrap());
    assert!(!store.unlink_command(mission_id, block_id, "s1").unwrap());
    assert!(store.get(block_id).is_ok());
    assert!(store.mission_commands(mission_id).unwrap().is_empty());
}

#[test]
fn missing_evidence_is_reported() {
    let store = Store::open_in_memory().unwrap();
    let mission_id = store.create_mission("M", "").unwrap();
    assert!(store.link_command(mission_id, 999, "s1", "user").unwrap());
    let cmds = store.mission_commands(mission_id).unwrap();
    assert_eq!(cmds.len(), 1);
    assert!(!cmds[0].available);
    assert!(store.mission_command_block(999, "s1").unwrap().is_none());
}

#[test]
fn cross_session_membership() {
    let store = Store::open_in_memory().unwrap();
    let mission_id = store.create_mission("Cross", "").unwrap();
    let b1 = store.append(&sample("s1")).unwrap();
    let b2 = store.append(&sample("s2")).unwrap();
    store.link_command(mission_id, b1, "s1", "user").unwrap();
    store.link_command(mission_id, b2, "s2", "user").unwrap();
    let cmds = store.mission_commands(mission_id).unwrap();
    assert_eq!(cmds.len(), 2);
}

#[test]
fn update_and_delete_mission() {
    let store = Store::open_in_memory().unwrap();
    let id = store.create_mission("Old", "desc").unwrap();
    assert!(store.update_mission(id, "New", "desc2").unwrap());
    let got = store.get_mission(id).unwrap().unwrap();
    assert_eq!(got.title, "New");
    assert_eq!(got.description, "desc2");
    assert!(store.delete_mission(id).unwrap());
    assert!(store.get_mission(id).unwrap().is_none());
    assert!(store.list_missions().unwrap().is_empty());
}

#[test]
fn link_requires_existing_mission() {
    let store = Store::open_in_memory().unwrap();
    let res = store.link_command(42, 1, "s1", "user");
    assert!(res.is_err());
}

#[test]
fn missions_for_block_reports_memberships() {
    let store = Store::open_in_memory().unwrap();
    let m1 = store.create_mission("A", "").unwrap();
    let m2 = store.create_mission("B", "").unwrap();
    let b = store.append(&sample("s1")).unwrap();
    store.link_command(m1, b, "s1", "user").unwrap();
    store.link_command(m2, b, "s1", "user").unwrap();
    let mut memberships = store.missions_for_block(b, "s1").unwrap();
    memberships.sort();
    assert_eq!(memberships, vec![m1, m2]);
}

#[test]
fn migration_adds_mission_tables_to_existing_db() {
    let path = std::env::temp_dir().join(format!("zet-mig-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        store.append(&sample("s1")).unwrap();
    }
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let id = store.create_mission("M", "").unwrap();
    assert!(id > 0);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn mission_persists_across_reopen() {
    let path =
        std::env::temp_dir().join(format!("zet-mission-persist-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let id = {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        let id = store.create_mission("Persist", "desc").unwrap();
        let b = store.append(&sample("s1")).unwrap();
        store.link_command(id, b, "s1", "user").unwrap();
        id
    };
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let m = store.get_mission(id).unwrap().unwrap();
    assert_eq!(m.title, "Persist");
    let cmds = store.mission_commands(id).unwrap();
    assert_eq!(cmds.len(), 1);
    assert!(cmds[0].available);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn legacy_database_migration() {
    let path = std::env::temp_dir().join(format!("zet-legacy-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    // Create an old-style database with only the blocks table (no git_branch, no missions).
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
                duration_ms INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO blocks (session, command, output, exit_code, cwd, duration_ms) VALUES ('s1','echo hi','hi',0,'/tmp',3)",
            [],
        )
        .unwrap();
    }
    let store = Store::open(path.to_str().unwrap()).unwrap();
    // git_branch column migrated; mission tables created.
    let b = store.list("s1").unwrap();
    assert_eq!(b.len(), 1);
    let id = store.create_mission("M", "").unwrap();
    let block_id = b[0].id;
    store.link_command(id, block_id, "s1", "user").unwrap();
    let cmds = store.mission_commands(id).unwrap();
    assert_eq!(cmds.len(), 1);
    assert!(cmds[0].available);
    std::fs::remove_file(&path).unwrap();
}
