use zet_pty::Session;

#[test]
fn spawn_echo_returns_output_and_exit_code() {
    let mut s = Session::spawn("sh", &["-c", "echo hello"], 80, 24).unwrap();
    let mut out = Vec::new();
    let code = s.wait_with_output(&mut out).unwrap();
    assert_eq!(code, 0);
    assert!(
        String::from_utf8_lossy(&out).contains("hello"),
        "output was: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn interactive_write_and_nonzero_exit() {
    let mut s = Session::spawn("sh", &[], 80, 24).unwrap();
    s.write_all(b"exit 42\n").unwrap();
    let mut out = Vec::new();
    let code = s.wait_with_output(&mut out).unwrap();
    assert_eq!(code, 42);
    assert_eq!(s.try_exit().unwrap(), Some(42));
}

#[test]
fn pty_output_feeds_term_grid() {
    use zet_term::Terminal;

    let mut s = Session::spawn("sh", &["-c", "printf 'hi\\n'"], 80, 24).unwrap();
    let mut out = Vec::new();
    s.wait_with_output(&mut out).unwrap();
    let mut t = Terminal::new(80, 24);
    t.feed(&out);
    assert_eq!(t.line_text(0).trim_end(), "hi");
}

#[test]
fn cloned_reader_streams_output_from_thread() {
    use std::io::Read;

    let s = Session::spawn("sh", &["-c", "echo from-thread"], 80, 24).unwrap();
    let mut reader = s.clone_reader().unwrap();
    let out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        buf
    })
    .join()
    .unwrap();
    assert!(String::from_utf8_lossy(&out).contains("from-thread"));
}

#[test]
fn consecutive_writes_all_reach_shell() {
    let mut s = Session::spawn("sh", &[], 80, 24).unwrap();
    s.write_all(b"echo ").unwrap();
    s.write_all(b"hi\n").unwrap();
    s.write_all(b"exit\n").unwrap();
    let mut out = Vec::new();
    s.wait_with_output(&mut out).unwrap();
    assert!(
        String::from_utf8_lossy(&out).contains("hi"),
        "output was: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
#[cfg(unix)]
fn erase_char_matches_line_discipline() {
    let s = Session::spawn("sh", &[], 80, 24).unwrap();
    let erase = s.erase_char();
    assert!(
        erase == 0x7f || erase == 0x08,
        "implausible VERASE {erase:#x}"
    );
}

#[test]
fn syntax_gate_parses_without_executing() {
    // Valid single and multi-line commands pass; the exact broken shape
    // a small model emitted live (missing ';' before done) fails.
    for cmd in [
        "echo hi",
        "echo 'agent-was-here' > /tmp/x",
        "if true; then\necho hi\nfi",
        "ls | head -5",
    ] {
        assert!(zet_pty::syntax_ok("bash", cmd), "{cmd:?}");
    }
    for cmd in [
        "for file in $(@); do echo \"$file\" done",
        "echo 'unterminated",
        "if true; then",
    ] {
        assert!(!zet_pty::syntax_ok("bash", cmd), "{cmd:?}");
    }
    // Nothing executes: a would-be destructive command leaves no trace.
    let probe = "/tmp/zet-syntax-probe";
    let _ = std::fs::remove_file(probe);
    assert!(zet_pty::syntax_ok("bash", &format!("touch {probe}")));
    assert!(!std::path::Path::new(probe).exists());
    // Uncheckable shells fail open (old behavior), never block.
    assert!(zet_pty::syntax_ok("nushell", "for $x in [] {}"));
    assert!(zet_pty::syntax_ok("/bin/nonexistent-sh-xyz", "rm -rf /"));
}

#[test]
fn shutdown_reaps_the_child() {
    // A sleeping shell must die and reap (no zombie): kill + wait.
    let mut s = Session::spawn("sh", &["-c", "sleep 30"], 80, 24).unwrap();
    let pid = s.child_id().expect("child pid");
    s.shutdown();
    assert!(s.try_exit().unwrap().is_some(), "child not reaped");
    // Truly gone, not zombie: kill -0 fails.
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "process {pid} lingers"
    );
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    for attempt in 0..1000 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "zet-cwd-{prefix}-{}-{nanos}-{attempt}",
            std::process::id()
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => return path,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => panic!("could not create isolated test directory {path:?}: {e}"),
        }
    }
    panic!("could not allocate a unique temporary directory");
}

#[test]
fn spawn_with_cwd_starts_shell_in_requested_directory() {
    let dir = unique_temp_dir("project");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let mut s = Session::spawn_with_cwd("sh", &["-c", "pwd"], 80, 24, Some(&dir)).unwrap();
    let mut out = Vec::new();
    let code = s.wait_with_output(&mut out).unwrap();
    assert_eq!(code, 0);

    let text = String::from_utf8_lossy(&out);
    let actual = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let actual = std::fs::canonicalize(actual).unwrap_or_else(|_| std::path::PathBuf::from(actual));
    let expected = std::fs::canonicalize(&dir).unwrap();
    assert_eq!(actual, expected, "pwd was {text:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn spawn_with_cwd_falls_back_when_directory_is_invalid() {
    let missing = std::env::temp_dir().join(format!(
        "zet-cwd-missing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    assert!(
        !missing.exists(),
        "test fixture unexpectedly exists: {missing:?}"
    );

    let mut s = Session::spawn_with_cwd("sh", &["-c", "pwd"], 80, 24, Some(&missing)).unwrap();
    let mut out = Vec::new();
    let code = s.wait_with_output(&mut out).unwrap();
    assert_eq!(code, 0);

    let text = String::from_utf8_lossy(&out);
    let actual = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    assert!(
        !actual.is_empty(),
        "shell did not report a working directory"
    );
    assert!(
        !actual.contains("zet-cwd-missing"),
        "invalid directory leaked into shell: {text:?}"
    );
}

#[test]
fn spawn_inherits_process_current_directory() {
    if std::env::var_os("ZET_PTY_INHERIT_CWD_CHILD").is_some() {
        let mut s = Session::spawn("sh", &["-c", "pwd"], 80, 24).unwrap();
        let mut out = Vec::new();
        let code = s.wait_with_output(&mut out).unwrap();
        assert_eq!(code, 0);
        let text = String::from_utf8_lossy(&out);
        let actual = text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default();
        println!("ZET_CWD_RESULT={actual}");
        return;
    }

    let dir = unique_temp_dir("inherit");
    let expected = std::fs::canonicalize(&dir).unwrap();
    let exe = std::env::current_exe().unwrap();
    let output = std::process::Command::new(&exe)
        .arg("spawn_inherits_process_current_directory")
        .arg("--exact")
        .arg("--nocapture")
        .env("ZET_PTY_INHERIT_CWD_CHILD", "1")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = stdout
        .lines()
        .find_map(|line| line.strip_prefix("ZET_CWD_RESULT="))
        .unwrap_or_default()
        .trim();
    let actual = std::fs::canonicalize(result).unwrap_or_else(|_| std::path::PathBuf::from(result));
    assert_eq!(actual, expected, "child pwd was {stdout:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
