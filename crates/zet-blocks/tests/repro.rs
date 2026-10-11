use std::io::Read;
use std::time::{Duration, Instant};
use zet_blocks::{Block, Recorder, Store};
use zet_term::Terminal;

// Full pipeline proof: real interactive bash + hook script -> PTY bytes ->
// Terminal -> Recorder -> Store. Ignored by default (spawns a shell, needs
// seconds); run with -- --ignored.

fn run_hooked_bash(args: &[&str], setup: Option<&str>) -> Vec<Block> {
    use zet_pty::Session;

    // One D marker per completed command: setup? + source + echo.
    let wait_ds = 2 + usize::from(setup.is_some());
    let mut s = Session::spawn("bash", args, 80, 24).unwrap();
    let hook = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../shell/zet-integration.bash"
    );
    if let Some(extra) = setup {
        s.write_all(format!("{extra}\n").as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(400));
    }
    s.write_all(format!("source {hook}\n").as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    s.write_all(b"echo hi\n").unwrap();

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

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut raw = Vec::new();
    let count_ds = |raw: &Vec<u8>| raw.windows(6).filter(|w| *w == b"133;D;").count();
    while Instant::now() < deadline {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
            raw.extend_from_slice(&chunk);
        }
        if count_ds(&raw) >= wait_ds {
            break;
        }
    }

    let store = Store::open_in_memory().unwrap();
    let mut rec = Recorder::new(store, "live");
    let mut t = Terminal::new(80, 24);
    t.feed(&raw);
    rec.pump(&mut t, "/tmp");
    let blocks = rec.blocks().unwrap();
    println!("BLOCKS: {blocks:?}");
    blocks
}

#[test]
#[ignore]
fn live_hooked_bash_records_block() {
    let blocks = run_hooked_bash(&["--noprofile", "--norc", "-i"], None);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].command, "echo hi");
    assert_eq!(blocks[0].output, "hi");
}

// Starship (and friends) rebuild PS1 every prompt, wiping statically
// appended markers. The hook must repair them via PROMPT_COMMAND.
#[test]
#[ignore]
fn live_starship_prompt_still_records() {
    if std::process::Command::new("starship")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("starship missing, skipping");
        return;
    }
    let blocks = run_hooked_bash(&["-i"], Some(r#"eval "$(starship init bash)""#));
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].command, "echo hi");
}
