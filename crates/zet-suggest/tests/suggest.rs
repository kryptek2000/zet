use std::cell::Cell;
use std::rc::Rc;
use zet_blocks::{Block, Store};
use zet_models::{ChatModel, ChatRequest, ChatResponse, ModelError};
use zet_suggest::{ChainSuggester, HistorySuggester, ModelSuggester, Suggest};

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

#[test]
fn history_completes_most_recent_success() {
    let store = seed(&[("git status", 0), ("git stash", 0), ("git bad", 1)]);
    let h = HistorySuggester::new(&store);
    assert_eq!(h.suggest("git s"), Some("git stash".to_string()));
    // Failed commands are never suggested.
    assert_eq!(h.suggest("git b"), None);
    // Empty prefix and exact matches yield nothing.
    assert_eq!(h.suggest(""), None);
    assert_eq!(h.suggest("git stash"), None);
}

#[test]
fn history_escapes_like_wildcards() {
    let store = seed(&[("100%_x", 0), ("100%_x extended", 0)]);
    let h = HistorySuggester::new(&store);
    assert_eq!(h.suggest("100%_x"), Some("100%_x extended".to_string()));
    assert_eq!(h.suggest("100"), Some("100%_x extended".to_string()));
}

#[test]
fn history_is_instant_on_10k_rows() {
    let store = Store::open_in_memory().unwrap();
    for i in 0..10_000 {
        store
            .append(&Block {
                id: 0,
                session: "s".to_string(),
                command: format!("run-{i:05} --flag value"),
                output: String::new(),
                exit_code: 0,
                cwd: String::new(),
                duration_ms: 1,
            })
            .unwrap();
    }
    let h = HistorySuggester::new(&store);
    let t0 = std::time::Instant::now();
    let got = h.suggest("run-099");
    let ms = t0.elapsed().as_millis();
    println!("10k-row suggest took {ms}ms");
    assert_eq!(got, Some("run-09999 --flag value".to_string()));
    assert!(ms < 100, "{ms}ms exceeds instant budget");
}

struct Script {
    reply: String,
    calls: Rc<Cell<usize>>,
}

impl ChatModel for Script {
    fn chat(&self, _req: &ChatRequest) -> Result<ChatResponse, ModelError> {
        self.calls.set(self.calls.get() + 1);
        Ok(ChatResponse {
            text: self.reply.clone(),
            tool_calls: vec![],
        })
    }
}

fn script(reply: &str) -> (Script, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    (
        Script {
            reply: reply.to_string(),
            calls: Rc::clone(&calls),
        },
        calls,
    )
}

#[test]
fn model_takes_first_command_line_only() {
    let (m, calls) = script("Sure thing!\ngit status\nDone.");
    let s = ModelSuggester::new(m);
    assert_eq!(
        s.suggest_nl("show git status"),
        Some("git status".to_string())
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn model_empty_is_none() {
    let (m, _) = script("\n  \n");
    let s = ModelSuggester::new(m);
    assert_eq!(s.suggest_nl("anything"), None);
}

#[test]
fn chain_prefers_history_over_model() {
    let store = seed(&[("kubectl get pods", 0)]);
    let (m, calls) = script("EXPENSIVE");
    let c = ChainSuggester::new(HistorySuggester::new(&store), ModelSuggester::new(m));
    assert_eq!(
        c.suggest("kubectl get"),
        Some("kubectl get pods".to_string())
    );
    assert_eq!(calls.get(), 0, "model must not run on history hit");
    let (m2, calls2) = script("from-model");
    let c2 = ChainSuggester::new(HistorySuggester::new(&store), ModelSuggester::new(m2));
    assert_eq!(c2.suggest_nl("list pods"), Some("from-model".to_string()));
    assert_eq!(calls2.get(), 1);
}

#[test]
fn model_strips_fences_and_bullets() {
    for (reply, want) in [
        ("```sh\ngit status\n```", "git status"),
        ("```git stash```", "git stash"),
        ("- kubectl get pods", "kubectl get pods"),
        ("$ ls -la", "ls -la"),
        ("```\n```", ""),
    ] {
        let (m, _) = script(reply);
        let s = ModelSuggester::new(m);
        assert_eq!(
            s.suggest_nl("x").as_deref(),
            if want.is_empty() { None } else { Some(want) },
            "{reply:?}"
        );
    }
}
