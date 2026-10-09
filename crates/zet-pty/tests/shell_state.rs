use zet_term::Event;

#[test]
fn prompt_command_cycle_tracks_exit_code() {
    use zet_pty::shell::ShellState;

    let mut st = ShellState::new();
    for body in ["A", "B", "C"] {
        st.observe(&Event::Osc {
            code: 133,
            body: body.to_string(),
        });
    }
    st.observe(&Event::Osc {
        code: 133,
        body: "D;2".to_string(),
    });
    assert_eq!(st.last_exit(), Some(2));
}

#[test]
fn phases_advance_and_foreign_codes_ignored() {
    use zet_pty::shell::{Phase, ShellState};

    let mut st = ShellState::new();
    assert_eq!(st.phase(), Phase::Prompt);
    assert_eq!(st.last_exit(), None);

    let osc = |code: u16, body: &str| Event::Osc {
        code,
        body: body.to_string(),
    };
    st.observe(&osc(133, "B"));
    assert_eq!(st.phase(), Phase::Input);
    st.observe(&osc(633, "C"));
    assert_eq!(st.phase(), Phase::Running);
    st.observe(&osc(999, "C"));
    assert_eq!(st.phase(), Phase::Running);
    st.observe(&osc(633, "D;7"));
    assert_eq!(st.phase(), Phase::Prompt);
    assert_eq!(st.last_exit(), Some(7));
}

#[test]
fn bare_d_preserves_prior_exit() {
    use zet_pty::shell::ShellState;

    let osc = |code: u16, body: &str| Event::Osc {
        code,
        body: body.to_string(),
    };
    let mut st = ShellState::new();
    st.observe(&osc(133, "D;5"));
    assert_eq!(st.last_exit(), Some(5));
    st.observe(&osc(133, "D"));
    assert_eq!(st.last_exit(), Some(5));
    st.observe(&osc(133, "D;bogus"));
    assert_eq!(st.last_exit(), Some(5));
}

#[test]
fn bash_hook_reports_exit_code_end_to_end() {
    use std::time::{Duration, Instant};
    use zet_pty::shell::ShellState;
    use zet_pty::Session;
    use zet_term::Terminal;

    let hook = format!(
        "{}/../../shell/zet-integration.bash",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut s = Session::spawn("bash", &["--norc", "-i"], 80, 24).unwrap();
    s.write_all(format!("source {hook}\nfalse\ntrue\nexit\n").as_bytes())
        .unwrap();

    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    // Poll: wait until the shell exits or time out, then drain output.
    let code = loop {
        if let Some(c) = s.try_exit().unwrap() {
            s.wait_with_output(&mut out).unwrap();
            break c;
        }
        if Instant::now() > deadline {
            panic!("bash did not exit in time");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(code, 0);

    let mut term = Terminal::new(80, 24);
    term.feed(&out);
    let mut st = ShellState::new();
    let mut exits = Vec::new();
    let mut saw_c = false;
    for e in term.drain_events() {
        if let zet_term::Event::Osc { code: 133, body } = &e {
            if let Some(code) = body.strip_prefix("D;") {
                exits.push(code.to_string());
            }
            if body == "C" {
                saw_c = true;
            }
        }
        st.observe(&e);
    }
    // `false` then `true`: both exits reported, tracker holds the latest.
    assert!(exits.contains(&"1".to_string()), "exits seen: {exits:?}");
    assert_eq!(exits.last().map(String::as_str), Some("0"));
    assert!(saw_c, "no pre-exec C marker observed");
    assert_eq!(st.last_exit(), Some(0));
    assert!(
        out.windows(4).any(|w| w == b"133;"),
        "no OSC markers in output"
    );
}

#[test]
fn zsh_hook_reports_exit_code_end_to_end() {
    use std::time::{Duration, Instant};
    use zet_pty::shell::ShellState;
    use zet_pty::Session;
    use zet_term::Terminal;

    if std::process::Command::new("zsh")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: zsh not installed");
        return;
    }
    // Isolate from the user's real zsh config.
    let zdotdir = std::env::temp_dir().join("zet-zsh-e2e");
    std::fs::create_dir_all(&zdotdir).unwrap();

    let hook = format!(
        "{}/../../shell/zet-integration.zsh",
        env!("CARGO_MANIFEST_DIR")
    );
    let zdotdir_arg = format!("ZDOTDIR={}", zdotdir.display());
    let mut s = Session::spawn("env", &[&zdotdir_arg, "zsh", "-i"], 80, 24).unwrap();
    s.write_all(format!("source {hook}\nfalse\ntrue\nexit\n").as_bytes())
        .unwrap();

    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    let code = loop {
        if let Some(c) = s.try_exit().unwrap() {
            s.wait_with_output(&mut out).unwrap();
            break c;
        }
        if Instant::now() > deadline {
            panic!("zsh did not exit in time");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(code, 0);

    let mut term = Terminal::new(80, 24);
    term.feed(&out);
    let mut st = ShellState::new();
    let mut exits = Vec::new();
    let mut saw_c = false;
    for e in term.drain_events() {
        if let zet_term::Event::Osc { code: 133, body } = &e {
            if let Some(code) = body.strip_prefix("D;") {
                exits.push(code.to_string());
            }
            if body == "C" {
                saw_c = true;
            }
        }
        st.observe(&e);
    }
    assert!(exits.contains(&"1".to_string()), "exits seen: {exits:?}");
    assert_eq!(exits.last().map(String::as_str), Some("0"));
    assert!(saw_c, "no pre-exec C marker observed");
    assert_eq!(st.last_exit(), Some(0));
}
