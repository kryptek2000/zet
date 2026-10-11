use std::io::Read;
use std::time::{Duration, Instant};
use zet_blocks::{Recorder, Store};
use zet_suggest::{Ghost, PromptInput};
use zet_term::Terminal;

// Headless ghost proof: real interactive bash + hooks, no window, no
// screenshots. Ignored by default (spawns a shell, needs seconds);
// run with -- --ignored. This is the fast path that keeps the GUI
// smoke pass to pixel-only checks.
#[test]
#[ignore]
fn live_partial_input_ghosts_history() {
    use zet_pty::Session;

    let mut s = Session::spawn("bash", &["--noprofile", "--norc", "-i"], 80, 24).unwrap();
    let hook = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../shell/zet-integration.bash"
    );
    s.write_all(format!("source {hook}\n").as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    // Seed history with a successful command.
    s.write_all(b"echo hello-smoke\n").unwrap();
    std::thread::sleep(Duration::from_millis(600));
    // Type a prefix WITHOUT Enter: the ghost must appear for it.
    s.write_all(b"echo hello-").unwrap();
    std::thread::sleep(Duration::from_millis(500));

    let mut reader = s.clone_reader().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut raw = Vec::new();
    while Instant::now() < deadline {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
            raw.extend_from_slice(&chunk);
        }
    }

    let store = Store::open_in_memory().unwrap();
    let mut rec = Recorder::new(store, "live-ghost");
    let mut t = Terminal::new(80, 24);
    t.feed(&raw);
    rec.pump(&mut t, "/tmp");

    // The seed command recorded (pipeline sanity).
    let blocks = rec.blocks().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].command, "echo hello-smoke");

    // The partial line ghosts its completion. The ghost owns its own
    // handle (as in the shell); seed it with what the shell recorded.
    let (text, col, abs) = rec.current_input(&t).expect("partial typing visible");
    assert_eq!(text, "echo hello-");
    let (cx, cy) = t.cursor();
    let sb = t.scrollback_len();
    let ghost_store = Store::open_in_memory().unwrap();
    for b in &blocks {
        ghost_store.append(b).unwrap();
    }
    let mut ghost = Ghost::new(ghost_store);
    ghost.update(Some(PromptInput {
        text,
        prompt_col: col,
        prompt_row: abs,
        cursor: (cx, sb + cy),
    }));
    assert_eq!(ghost.visible(), Some("smoke"));
    assert!(ghost.tabbable());
}
