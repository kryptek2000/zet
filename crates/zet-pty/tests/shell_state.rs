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
    let hook = format!(
        "{}/../../shell/zet-integration.zsh",
        env!("CARGO_MANIFEST_DIR")
    );

    // Isolate from the user's real zsh config. The scratch dir is keyed by
    // thread id, not pid alone: cargo runs tests in parallel threads, and a
    // pid-keyed dir lets one test's cleanup wipe another test's ZDOTDIR.
    let zdotdir = std::env::temp_dir().join(format!(
        "zet-zsh-e2e-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&zdotdir).unwrap();

    // Load the hook from the rc file, never by typing `source` at a prompt.
    // A fresh interactive zsh runs the system-wide /etc/zsh/zshrc, which
    // calls compinit; compinit raises an interactive "Ignore insecure
    // directories and continue [y] or abort compinit [n]?" prompt that reads
    // the same stdin as this test. The prompt eats the first byte of the next
    // line, so a typed `source` arrives as `ource` and the hook never loads
    // ("zsh: command not found: ource" -- no OSC 133 markers, exit codes
    // unseen). rc loading removes the setup line from the prompt stream
    // entirely, so nothing typed later can be eaten.
    let _ = std::fs::write(zdotdir.join(".zshrc"), format!("source {hook}\n"));

    let zdotdir_arg = format!("ZDOTDIR={}", zdotdir.display());
    // -d (--no-globalrcs) skips the system-wide /etc/zsh/zshrc, so the
    // compinit security prompt never appears at all. The user's own rc
    // ($ZDOTDIR/.zshrc, written above) is still read -- -d only drops the
    // GLOBAL rc files, so the hook still loads.
    let mut s = Session::spawn("env", &[&zdotdir_arg, "zsh", "-d", "-i"], 80, 24).unwrap();
    s.write_all(b"false\ntrue\nexit\n").unwrap();

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
    assert!(
        exits.contains(&"1".to_string()),
        "exits seen: {exits:?}; raw: {}",
        String::from_utf8_lossy(&out)
    );
    assert_eq!(exits.last().map(String::as_str), Some("0"));
    assert!(saw_c, "no pre-exec C marker observed");
    assert_eq!(st.last_exit(), Some(0));
}

/// The hook must not spam the prompt when zsh/parameter is unavailable.
///
/// The `${+aliases}` parameter expansion re-triggers zsh's module load on
/// every prompt. When the module is missing, that re-trigger printed
/// "failed to load module `zsh/parameter'" to stderr on every single
/// prompt — a permanently noisy shell for anyone whose zsh lacks the
/// module. The hook caches the load result once and gates the expansion
/// on it, so the prompt stays silent.
///
/// Runs non-interactively so zsh/zle is never needed (no line editor),
/// and forces module_path to an empty directory so zsh/parameter cannot
/// load. precmd is invoked explicitly to exercise the expansion path.
#[test]
fn zsh_hook_is_quiet_when_parameter_module_is_missing() {
    use std::process::Command;

    if Command::new("zsh").arg("--version").output().is_err() {
        eprintln!("skipping: zsh not installed");
        return;
    }
    let hook = format!(
        "{}/../../shell/zet-integration.zsh",
        env!("CARGO_MANIFEST_DIR")
    );

    // Empty module dir: zsh/parameter (and everything else) is unfindable.
    let empty_mods = std::env::temp_dir().join(format!(
        "zet-zsh-emptymods-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&empty_mods);
    std::fs::create_dir_all(empty_mods.join("zsh")).unwrap();

    // Source the hook, then call precmd directly. Capture stderr.
    let script = format!(
        "module_path=('{}')\nsource {}\n__zet_precmd\n",
        empty_mods.display(),
        hook
    );
    let out = Command::new("zsh")
        .args(["-f", "-c", &script])
        .output()
        .expect("zsh");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("failed to load module"),
        "hook printed a module-load error: {stderr}"
    );
}
