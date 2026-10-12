use zet_pty::{fix_path_case, levenshtein, suggest_fix, KnownCommands};

fn known_with(extra: &[&str]) -> KnownCommands {
    let mut k = KnownCommands::from_env();
    k.insert_names(extra.iter().copied());
    k
}

#[test]
fn levenshtein_basics() {
    assert_eq!(levenshtein("", ""), 0);
    assert_eq!(levenshtein("gut", "git"), 1);
    assert_eq!(levenshtein("kitten", "sitting"), 3);
    assert_eq!(levenshtein("abc", ""), 3);
    assert_eq!(levenshtein("same", "same"), 0);
}

#[test]
fn closest_repairs_typos() {
    // Hermetic: a long injected name has no PATH neighbors, so the
    // correction is deterministic on every machine.
    let k = known_with(&["zzq-typo-target"]);
    // exit 127, unknown first word, close to a known command.
    assert_eq!(
        suggest_fix(
            "zzq-typo-targe --flag",
            "/tmp",
            "zzq-typo-targe: command not found",
            127,
            &k
        ),
        Some("zzq-typo-target --flag".to_string())
    );
    // Known words pass through untouched (no offer).
    assert_eq!(suggest_fix("git status", "/tmp", "", 1, &k), None);
    // Nothing close: silence, not noise (nonsense word: hermetic).
    assert_eq!(
        suggest_fix(
            "zzq-hello-word",
            "/tmp",
            "zzq-hello-word: command not found",
            127,
            &k
        ),
        None
    );
    // Far typos stay silent.
    assert_eq!(
        suggest_fix(
            "zzq-typo-tarXXXX",
            "/tmp",
            "zzq-typo-tarXXXX: command not found",
            127,
            &k
        ),
        None
    );
}

#[test]
fn case_fixes_directory_names() {
    let dir = std::env::temp_dir().join(format!(
        "zet-fix-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Pictures")).unwrap();
    let cwd = dir.to_str().unwrap();
    // ls failure output, lowercase attempt.
    let out = "ls: cannot access 'pictures/': No such file or directory";
    assert_eq!(
        suggest_fix("ls pictures/", cwd, out, 2, &known_with(&[])),
        Some("ls Pictures/".to_string())
    );
    // Double-quoted variant (some tools).
    let out2 = "\"pictures/\": No such file or directory";
    assert_eq!(
        suggest_fix("ls pictures/", cwd, out2, 1, &known_with(&[])),
        Some("ls Pictures/".to_string())
    );
    // Ambiguous (two case variants): silence.
    std::fs::create_dir_all(dir.join("PICTURES")).unwrap();
    assert_eq!(
        suggest_fix("ls pictures/", cwd, out, 2, &known_with(&[])),
        None
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fix_path_case_handles_files_and_missing() {
    let dir = std::env::temp_dir().join(format!(
        "zet-fix2-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Readme.md"), "x").unwrap();
    let cwd = dir.to_str().unwrap();
    assert_eq!(
        fix_path_case(cwd, "readme.md"),
        Some("Readme.md".to_string())
    );
    // No match: None.
    assert_eq!(fix_path_case(cwd, "nope.txt"), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn successes_and_empty_never_offer() {
    let k = known_with(&["git"]);
    assert_eq!(suggest_fix("git status", "/tmp", "", 0, &k), None);
    assert_eq!(suggest_fix("", "/tmp", "", 1, &k), None);
    // Exit 127 but the word IS known (moved binary, not a typo): silent.
    assert_eq!(
        suggest_fix("git status", "/tmp", "git: command not found", 127, &k),
        None
    );
}
