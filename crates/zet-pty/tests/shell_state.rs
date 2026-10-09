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
    for e in term.drain_events() {
        if let zet_term::Event::Osc { code: 133, body } = &e {
            if let Some(code) = body.strip_prefix("D;") {
                exits.push(code.to_string());
            }
        }
        st.observe(&e);
    }
    // `false` then `true`: both exits reported, tracker holds the latest.
    assert!(exits.contains(&"1".to_string()), "exits seen: {exits:?}");
    assert_eq!(exits.last().map(String::as_str), Some("0"));
    assert_eq!(st.last_exit(), Some(0));
    assert!(
        out.windows(4).any(|w| w == b"133;"),
        "no OSC markers in output"
    );
}
