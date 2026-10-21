//! One state root: every zet file lives under `$ZET_HOME` when that is
//! set, and under `$HOME/.zet` only when it is not. The window and the
//! CLI share that root, so `ZET_HOME` relocates a whole machine's state
//! in one move — and, just as important, nothing lands in the old
//! place.
//!
//! This file is that guarantee. It drives the real binary with both
//! roots pointed at scratch directories and checks where the files
//! land: under `$ZET_HOME`, and nowhere else.

use std::path::{Path, PathBuf};
use std::process::Command;

fn zet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_zet"))
}

/// Scratch roots for one scenario. `$HOME` and `$ZET_HOME` both point
/// inside a fresh temp directory, so every file the binary writes is
/// attributable to one of the two — and the difference between them is
/// exactly what these tests measure.
struct Roots {
    home: PathBuf,
    zethome: PathBuf,
}

impl Roots {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base =
            std::env::temp_dir().join(format!("zet_state_{tag}_{}_{nanos}", std::process::id()));
        let home = base.join("home");
        let zethome = base.join("zethome");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&zethome).unwrap();
        Roots { home, zethome }
    }

    /// The binary, with the state root relocated.
    fn cmd(&self) -> Command {
        let mut cmd = zet();
        cmd.env("HOME", &self.home)
            .env("ZET_HOME", &self.zethome)
            .env_remove("ZET_SESSION");
        cmd
    }

    /// The old root: where state used to land, and must not land now.
    fn home_zet(&self) -> PathBuf {
        self.home.join(".zet")
    }
}

/// Every file under `dir`, recursively, sorted — the "what did this
/// actually write?" view.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn inline_spec_lands_under_zet_home() {
    let roots = Roots::new("inline");
    let out = roots
        .cmd()
        .args(["inline", "local:llama3.2:1b"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    let spec = roots.zethome.join("inline_spec");
    assert!(
        spec.exists(),
        "inline_spec not under ZET_HOME: {:?}",
        files_under(&roots.zethome)
    );
    assert_eq!(
        std::fs::read_to_string(&spec).unwrap().trim(),
        "local:llama3.2:1b"
    );

    // Read back: the file under ZET_HOME is the one the CLI reads.
    let out = roots.cmd().arg("inline").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("(set via `zet inline`)"), "{text}");

    // `off` clears it — from the relocated root.
    let out = roots.cmd().args(["inline", "off"]).output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    assert!(!spec.exists(), "inline spec survived `zet inline off`");
    assert!(
        !roots.home_zet().exists(),
        "state leaked into $HOME/.zet: {:?}",
        files_under(&roots.home)
    );
}

#[test]
fn schedules_land_under_zet_home() {
    let roots = Roots::new("schedules");
    // A dead endpoint keeps the turn hermetic and fast: the schedule is
    // recorded before the turn runs, and the turn fails on connect.
    let out = roots
        .cmd()
        .env("ZET_INLINE_SPEC", "local:http://127.0.0.1:9/dead")
        .args(["run", "--every", "1h", "say hi"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("scheduled"), "{stdout}");
    let sched = roots.zethome.join("schedules.jsonl");
    let text = std::fs::read_to_string(&sched)
        .unwrap_or_else(|e| panic!("no schedules.jsonl under ZET_HOME: {e}"));
    assert!(text.contains("say hi"), "{text}");
    assert!(text.contains("\"every_secs\":3600"), "{text}");
    assert!(!roots.home_zet().join("schedules.jsonl").exists());
}

#[test]
fn the_block_store_lands_under_zet_home() {
    let roots = Roots::new("blocks");
    // The CLI opens the default store: it must be the one under
    // ZET_HOME, and opening it must not touch $HOME/.zet.
    let out = roots.cmd().args(["blocks"]).output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    let db = roots.zethome.join("blocks.sqlite");
    assert!(
        db.exists(),
        "no blocks.sqlite under ZET_HOME: {:?}",
        files_under(&roots.zethome)
    );
    assert!(!roots.home_zet().exists(), "store leaked into $HOME/.zet");

    // A block written there is the block the CLI lists.
    let store = zet_blocks::Store::open(db.to_str().unwrap()).unwrap();
    store
        .append(&zet_blocks::Block {
            id: 0,
            session: "s1".to_string(),
            command: "echo alpha".to_string(),
            output: "alpha\n".to_string(),
            exit_code: 0,
            cwd: "/tmp".to_string(),
            git_branch: String::new(),
            duration_ms: 3,
        })
        .unwrap();
    drop(store);
    let out = roots.cmd().args(["blocks"]).output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("echo alpha"), "{text}");
    assert!(!roots.home_zet().exists(), "store leaked into $HOME/.zet");
}

#[test]
fn journal_and_usage_come_from_zet_home() {
    let roots = Roots::new("journal_usage");
    zet_agent::append_journal(
        &roots.zethome.join("journal.jsonl"),
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
    let out = roots.cmd().args(["journal"]).output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("echo agent-step"), "{text}");

    std::fs::write(
        roots.zethome.join("usage.jsonl"),
        "{\"ts\":1,\"label\":\"opencode:spark\",\"in_chars\":400,\"out_chars\":400,\
         \"in_est\":100,\"out_est\":100,\"ms\":50}\n",
    )
    .unwrap();
    let out = roots.cmd().arg("usage").output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("1 calls"), "{text}");
    assert!(!roots.home_zet().exists(), "state leaked into $HOME/.zet");
}

#[test]
fn recipes_dir_is_under_zet_home() {
    let roots = Roots::new("recipes");
    let dir = roots.zethome.join("recipes");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("greet.md"), "Say hello").unwrap();
    // The error names the recipes it found: "have: greet" means the CLI
    // read the relocated dir, not an empty $HOME/.zet/recipes.
    let out = roots.cmd().args(["run", "recipe:nope"]).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("have: greet"), "{err}");
}

#[test]
fn nothing_lands_under_home_zet() {
    let roots = Roots::new("one_root");
    // Every writer in the tree, one after another: the inline spec, the
    // schedule, the block store, the journal, the usage log, recipes.
    let _ = roots
        .cmd()
        .args(["inline", "local:llama3.2:1b"])
        .output()
        .unwrap();
    let _ = roots.cmd().args(["inline", "off"]).output().unwrap();
    let _ = roots
        .cmd()
        .env("ZET_INLINE_SPEC", "local:http://127.0.0.1:9/dead")
        .args(["run", "--every", "1h", "say hi"])
        .output()
        .unwrap();
    let _ = roots.cmd().args(["blocks"]).output().unwrap();
    let _ = roots.cmd().args(["journal"]).output().unwrap();
    let _ = roots.cmd().arg("usage").output().unwrap();
    let _ = roots.cmd().args(["schedules"]).output().unwrap();
    let _ = roots.cmd().args(["runs"]).output().unwrap();
    let _ = roots.cmd().args(["run", "recipe:nope"]).output().unwrap();

    // The old root is untouched: not one file, not one directory.
    assert!(
        !roots.home_zet().exists(),
        "state leaked into $HOME/.zet: {:?}",
        files_under(&roots.home)
    );
    // And the state did land: one root, holding the files.
    let files = files_under(&roots.zethome);
    assert!(
        files.iter().any(|f| f.ends_with("blocks.sqlite")),
        "{files:?}"
    );
    assert!(
        files.iter().any(|f| f.ends_with("schedules.jsonl")),
        "{files:?}"
    );
}
