use zet_blocks::{Block, Store};
use zet_suggest::{Ghost, PromptInput};

fn seed(cmds: &[(&str, i32)]) -> Store {
    let store = Store::open_in_memory().unwrap();
    for (cmd, exit) in cmds {
        store
            .append(&Block {
                id: 0,
                session: "s".to_string(),
                command: cmd.to_string(),
                output: String::new(),
                exit_code: *exit,
                cwd: String::new(),
                duration_ms: 1,
            })
            .unwrap();
    }
    store
}

fn input(text: &str) -> Option<PromptInput> {
    // Cursor parked at the end of single-row input at prompt col 2.
    Some(PromptInput {
        text: text.to_string(),
        prompt_col: 2,
        prompt_row: 10,
        cursor: (2 + text.chars().count(), 10),
    })
}

#[test]
fn ghost_shows_suffix_and_accepts() {
    let mut g = Ghost::new(seed(&[("echo hello-smoke", 0)]));
    g.update(input("echo hello-"));
    assert_eq!(g.visible(), Some("smoke"));
    assert!(g.tabbable());
    assert_eq!(accept_bytes(&mut g), Some("smoke".to_string()));
    assert_eq!(g.visible(), None);
    // Completed text hides the ghost.
    g.update(input("echo hello-smoke"));
    assert_eq!(g.visible(), None);
}

fn accept_bytes(g: &mut Ghost) -> Option<String> {
    g.accept()
}

#[test]
fn ghost_hides_on_exact_failed_and_empty() {
    let mut g = Ghost::new(seed(&[("git status", 0), ("git bad", 1)]));
    g.update(input("git status"));
    assert_eq!(g.visible(), None);
    g.update(input("git b"));
    assert_eq!(g.visible(), None, "failed commands never ghost");
    g.update(input(""));
    assert_eq!(g.visible(), None);
    g.update(None);
    assert_eq!(g.visible(), None);
    assert!(!g.tabbable());
}

#[test]
fn ghost_hides_when_cursor_leaves_end() {
    let mut g = Ghost::new(seed(&[("echo hello-smoke", 0)]));
    g.update(Some(PromptInput {
        text: "echo hello-".to_string(),
        prompt_col: 2,
        prompt_row: 10,
        cursor: (5, 10),
    }));
    assert_eq!(g.visible(), None);
    assert!(!g.tabbable());
}

#[test]
fn cancel_dismisses_until_input_changes() {
    let mut g = Ghost::new(seed(&[("echo hello-smoke", 0)]));
    g.update(input("echo hello-"));
    assert_eq!(g.visible(), Some("smoke"));
    assert!(g.cancel(), "swallow: something was showing");
    assert_eq!(g.visible(), None);
    assert!(!g.cancel(), "pass through: nothing showing");
    g.update(input("echo hello-"));
    assert_eq!(g.visible(), None, "still dismissed for same input");
    g.update(input("echo hello-s"));
    assert_eq!(g.visible(), Some("moke"), "new input re-suggests");
}

#[test]
fn notice_is_sticky_but_never_typed() {
    let mut g = Ghost::new(seed(&[("echo hello-smoke", 0)]));
    g.notify(
        "list files".to_string(),
        "(set ZET_INLINE_SPEC for NL)".to_string(),
    );
    assert_eq!(g.visible(), Some("(set ZET_INLINE_SPEC for NL)"));
    assert!(!g.tabbable());
    assert_eq!(g.accept(), None, "Tab must not type the notice");
    g.update(input("list files"));
    assert_eq!(g.visible(), Some("(set ZET_INLINE_SPEC for NL)"), "sticky");
    assert!(g.cancel(), "Esc swallows the notice");
    assert_eq!(g.visible(), None);
    g.update(input("list files"));
    assert_eq!(g.visible(), None, "cancelled notice never resurrects");
    g.notify("y".to_string(), "hint".to_string());
    g.update(input("y2"));
    assert_eq!(g.visible(), None, "input change clears the notice");
}

#[test]
fn ghost_truncates_multiline_history() {
    let mut g = Ghost::new(seed(&[("foo\nbar", 0)]));
    g.update(input("fo"));
    assert_eq!(g.visible(), Some("o"));
    g.update(input("foo"));
    assert_eq!(g.visible(), None, "first line exhausted");
}

#[test]
fn reset_restores_fresh_context() {
    let mut g = Ghost::new(seed(&[("echo hello-smoke", 0)]));
    g.update(input("echo hello-"));
    assert!(g.cancel());
    g.reset();
    g.update(input("echo hello-"));
    assert_eq!(g.visible(), Some("smoke"), "reset clears dismissal");
}

#[test]
fn dismiss_current_suppresses_without_visible_ghost() {
    let mut g = Ghost::new(seed(&[("echo hello-smoke", 0)]));
    g.update(input("echo hello-s"));
    assert_eq!(g.visible(), Some("moke"));
    // Esc in NL mode with nothing showing (simulate: cancel first).
    assert!(g.cancel());
    g.update(input("echo hello-s"));
    assert_eq!(g.visible(), None);
    // Same via dismiss_current directly (no ghost required).
    g.reset();
    g.update(input("echo hello-"));
    g.dismiss_current();
    g.update(input("echo hello-"));
    assert_eq!(g.visible(), None);
}

#[test]
fn no_command_notice_is_sticky() {
    let mut g = Ghost::new(seed(&[("echo hello-smoke", 0)]));
    g.notify("do stuff".to_string(), "(no command)".to_string());
    g.update(input("do stuff"));
    assert_eq!(g.visible(), Some("(no command)"));
    assert_eq!(g.accept(), None);
}

#[test]
fn redacted_suggestions_never_show() {
    // The store scrubs secrets to [REDACTED]; ghosting the literal
    // would type a placeholder and imply hidden knowledge.
    let mut g = Ghost::new(seed(&[("export K=[REDACTED]", 0)]));
    g.update(input("export K="));
    assert_eq!(g.visible(), None);
    assert!(!g.tabbable());
    assert_eq!(g.accept(), None);
}
