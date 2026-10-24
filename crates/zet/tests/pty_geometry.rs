//! Real PTY and viewport geometry checks for the future panel layouts.
//!
//! These tests do not render. They verify that a real shell reports the
//! same dimensions ZET resolves and stores in its terminal grid.

use zet::window::layout;
use zet_pty::Session;
use zet_term::Terminal;

#[test]
fn workspace_viewport_matches_real_pty_and_grid() {
    for bar_rows in [0usize, 1] {
        let (cols, rows) =
            layout::terminal_viewport(layout::LayoutMode::Workspace.layout(), 104, 24, bar_rows);
        assert_eq!((cols, rows), (80, if bar_rows == 0 { 24 } else { 23 }));

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
