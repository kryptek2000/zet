//! Real PTY and viewport geometry checks for the future panel layouts.
//!
//! These tests do not render. They verify that a real shell reports the
//! same dimensions ZET resolves and stores in its terminal grid.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use zet::window::layout;
use zet_pty::Session;
use zet_term::Terminal;

#[test]
fn workspace_viewport_matches_real_pty_and_grid() {
    for bar_rows in [0usize, 3] {
        let (cols, rows) =
            layout::terminal_viewport(layout::LayoutMode::Workspace.layout(), 104, 24, bar_rows);
        assert_eq!((cols, rows), (82, if bar_rows == 0 { 24 } else { 21 }));

        let mut session = Session::spawn("stty", &["size"], cols as u16, rows as u16).unwrap();
        let mut output = Vec::new();
        let status = session.wait_with_output(&mut output).unwrap();
        assert_eq!(status, 0);
        let text = String::from_utf8_lossy(&output);
        assert_eq!(text.trim(), format!("{rows} {cols}"), "pty={text}");

        let mut term = Terminal::new(cols as usize, rows as usize);
        assert_eq!((term.cols(), term.rows()), (cols as usize, rows as usize));
        term.resize(cols as usize, rows as usize);
        assert_eq!((term.cols(), term.rows()), (cols as usize, rows as usize));
    }
}

#[test]
fn terminal_focus_remains_full_window() {
    let (cols, rows) =
        layout::terminal_viewport(layout::LayoutMode::TerminalFocus.layout(), 104, 24, 1);
    assert_eq!((cols, rows), (104, 23));
}

#[test]
fn real_pty_observes_resize() {
    let mut session = Session::spawn("sleep", &["30"], 80, 24).unwrap();
    assert_eq!(session.size().unwrap(), (80, 24));
    session.resize(80, 23).unwrap();
    assert_eq!(session.size().unwrap(), (80, 23));
    session.resize(104, 24).unwrap();
    assert_eq!(session.size().unwrap(), (104, 24));
    session.shutdown();
}

fn wait_for(output: &Arc<Mutex<String>>, needle: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if output.lock().unwrap().contains(needle) {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn real_pty_child_observes_resize() {
    let mut session = Session::spawn(
        "sh",
        &["-c", "while :; do stty size; sleep 0.05; done"],
        80,
        24,
    )
    .unwrap();
    let reader = session.clone_reader().unwrap();
    let output = Arc::new(Mutex::new(String::new()));
    let collector = {
        let output = Arc::clone(&output);
        thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 256];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]);
                        output.lock().unwrap().push_str(&text);
                    }
                    Err(_) => break,
                }
            }
        })
    };

    assert!(
        wait_for(&output, "24 80", Duration::from_secs(2)),
        "{}",
        output.lock().unwrap()
    );
    assert!(session.try_exit().unwrap().is_none());

    session.resize(80, 23).unwrap();
    assert!(
        wait_for(&output, "23 80", Duration::from_secs(2)),
        "{}",
        output.lock().unwrap()
    );
    assert!(session.try_exit().unwrap().is_none());

    session.resize(104, 24).unwrap();
    assert!(
        wait_for(&output, "24 104", Duration::from_secs(2)),
        "{}",
        output.lock().unwrap()
    );
    assert!(session.try_exit().unwrap().is_none());

    session.shutdown();
    let _ = collector.join();
}
