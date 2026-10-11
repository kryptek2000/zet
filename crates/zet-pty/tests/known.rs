use zet_pty::{first_word, is_shell_word, KnownCommands};

// --- first_word: the command position, past assignments ---

#[test]
fn plain_command() {
    assert_eq!(first_word("ls -la"), Some("ls".to_string()));
}

#[test]
fn leading_space_and_empty() {
    assert_eq!(first_word("   git status"), Some("git".to_string()));
    assert_eq!(first_word(""), None);
    assert_eq!(first_word("   "), None);
}

#[test]
fn env_assignments_skipped() {
    assert_eq!(first_word("FOO=1 BAR=x ls -la"), Some("ls".to_string()));
    // A bare assignment still runs (sets the var): shell word, no command.
    assert_eq!(first_word("FOO=1"), None);
}

#[test]
fn quotes_and_sudo() {
    assert_eq!(first_word("\"my cmd\" arg"), Some("my cmd".to_string()));
    assert_eq!(first_word("sudo apt update"), Some("sudo".to_string()));
}

#[test]
fn keywords_run_directly() {
    for kw in ["if", "for", "while", "function", "time", "do", "then"] {
        assert!(is_shell_word(kw), "{kw}");
    }
    assert!(!is_shell_word("hello"));
    assert!(!is_shell_word("update"));
}

#[test]
fn builtins_known() {
    for b in ["cd", "exit", "export", "echo", "alias", "source"] {
        assert!(is_shell_word(b), "{b}");
    }
}

// --- KnownCommands: PATH + dynamic shell names ---

#[test]
fn path_scan_knows_sh() {
    let known = KnownCommands::from_env();
    // `sh` is on PATH on every unix we run on.
    assert!(known.contains("sh"));
    assert!(!known.contains("definitely-not-a-command-xyz"));
}

#[test]
fn dynamic_names_cover_aliases() {
    let mut known = KnownCommands::from_env();
    assert!(!known.contains("ll"));
    known.insert_names(["ll", "gco"]);
    assert!(known.contains("ll"));
    assert!(known.contains("gco"));
}

#[test]
fn first_word_gates_routing() {
    // The window routes: known first word runs, unknown translates.
    let mut known = KnownCommands::from_env();
    known.insert_names(["ll"]);
    let route = |line: &str| match first_word(line) {
        None => "run",
        Some(w) if known.contains(&w) || is_shell_word(&w) => "run",
        _ => "translate",
    };
    assert_eq!(route("ls -la"), "run");
    assert_eq!(route("  FOO=1 ll"), "run");
    assert_eq!(route("if true; then"), "run");
    assert_eq!(route("hello"), "translate");
    assert_eq!(route("update"), "translate");
    assert_eq!(route("list files in detail"), "translate");
}
