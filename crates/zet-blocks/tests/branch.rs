// Branch capture: the recorder stamps every finished block with the
// branch it ran on. Best effort, so the failure cases matter as much as
// the happy one — a missing git must never cost a recorded command.

use std::path::Path;
use std::process::Command;

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("zet-branch-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn branch_is_read_from_the_repo_at_the_blocks_cwd() {
    if !git_available() {
        eprintln!("git missing, skipping");
        return;
    }
    let dir = temp_dir("repo");
    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    git(&dir, &["checkout", "-q", "-b", "testbranch"]);
    std::fs::write(dir.join("f.txt"), "v1").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "v1"]);

    assert_eq!(zet_blocks::git_branch(dir.to_str().unwrap()), "testbranch");
    // Detached HEAD has no branch name: empty, not a stale one.
    git(&dir, &["checkout", "-q", "--detach"]);
    assert_eq!(zet_blocks::git_branch(dir.to_str().unwrap()), "");
    git(&dir, &["checkout", "-q", "testbranch"]);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn outside_a_repo_the_branch_is_empty() {
    if !git_available() {
        eprintln!("git missing, skipping");
        return;
    }
    // A plain temp dir: no .git anywhere above it, and nothing to record.
    let dir = temp_dir("plain");
    assert_eq!(zet_blocks::git_branch(dir.to_str().unwrap()), "");
    // A path that does not exist at all is equally fine (deleted cwd).
    let gone = dir.join("nope");
    assert_eq!(zet_blocks::git_branch(gone.to_str().unwrap()), "");
    std::fs::remove_dir_all(&dir).unwrap();
}
