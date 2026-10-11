use zet_models::keys::{EnvKeychain, Keychain, SystemKeychain};

const PROVIDERS: &[&str] = &["openai", "openrouter", "compat"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match argv.as_slice() {
        ["auth", "show"] => auth_show(),
        ["auth", "add", provider] => auth_add(provider),
        ["auth", "rm", provider] => auth_rm(provider),
        ["blocks", rest @ ..] => blocks(rest),
        ["journal", rest @ ..] => journal(rest),
        _ => {
            eprintln!("usage:");
            eprintln!("  zet auth show             list providers and key status");
            eprintln!("  zet auth add <provider>   prompt for key, store in OS keychain");
            eprintln!("  zet auth rm <provider>    delete key from OS keychain");
            eprintln!("  zet blocks [--db PATH] [--limit N] [--session S]");
            eprintln!("                            list recorded command blocks");
            eprintln!("  zet journal [--limit N]   list agent turn journal entries");
            std::process::exit(2);
        }
    }
}

fn auth_show() {
    for provider in PROVIDERS {
        let status = if EnvKeychain.get(provider).is_some() {
            "set (env)"
        } else if SystemKeychain.get(provider).is_some() {
            "set (keychain)"
        } else {
            "missing"
        };
        println!("{provider}: {status}");
    }
}

fn auth_add(provider: &str) {
    if !PROVIDERS.contains(&provider) {
        eprintln!("unknown provider {provider:?}: want one of {PROVIDERS:?}");
        std::process::exit(2);
    }
    // First line only: read_to_string blocks to EOF on a terminal and would
    // swallow later input into the key. Stdin::lines is inherent.
    // Echo off: a pasted key must never echo onto the PTY grid, where the
    // shell hooks would record it into blocks (seen live, since fixed).
    eprint!("Paste API key for {provider}: ");
    use std::io::Write as _;
    let _ = std::io::stderr().flush();
    let key = match read_secret() {
        Some(line) => line,
        _ => {
            eprintln!("failed to read key from stdin");
            std::process::exit(1);
        }
    };
    let key = key.trim();
    if key.is_empty() {
        eprintln!("empty key, not stored");
        std::process::exit(1);
    }
    match SystemKeychain.set(provider, key) {
        Ok(()) => println!("{provider}: stored in OS keychain"),
        Err(e) => {
            eprintln!("{provider}: not stored: {e}");
            std::process::exit(1);
        }
    }
}

/// One secret line from stdin with terminal echo disabled (unix TTY),
/// restored on drop. Piped stdin skips the termios dance (no echo there).
/// Ctrl+C aborts via EINTR (std's line reader would retry it forever).
fn read_secret() -> Option<String> {
    #[cfg(unix)]
    {
        let stdin = std::io::stdin();
        // Bound, not inline: a temporary would drop (restoring echo)
        // before the read below.
        let _guard = NoEcho::disable(&stdin);
        if _guard.is_some() {
            let line = read_line_raw(&stdin)?;
            eprintln!();
            return Some(line);
        }
    }
    let line = std::io::stdin().lines().next()?.ok()?;
    eprintln!();
    Some(line)
}

/// Byte-at-a-time first line; EINTR (our Ctrl+C handler, no SA_RESTART)
/// aborts to None instead of retrying. Strips one trailing CR for CRLF
/// pastes; empty/EOF yields Some("") and the caller reports it.
#[cfg(unix)]
fn read_line_raw(stdin: &std::io::Stdin) -> Option<String> {
    use std::os::unix::io::{AsFd as _, AsRawFd as _};
    let fd = stdin.as_fd().as_raw_fd();
    let mut buf = Vec::new();
    let mut one = [0u8; 1];
    loop {
        match nix::unistd::read(fd, &mut one) {
            Ok(0) => break,
            Ok(_) => {
                if one[0] == b'\n' {
                    break;
                }
                buf.push(one[0]);
            }
            Err(_) => return None,
        }
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8(buf).ok()
}

#[cfg(unix)]
struct NoEcho<'a> {
    fd: std::os::fd::BorrowedFd<'a>,
    orig: nix::sys::termios::Termios,
    old_sigint: nix::sys::signal::SigAction,
}

#[cfg(unix)]
impl<'a> NoEcho<'a> {
    fn disable(stdin: &'a std::io::Stdin) -> Option<Self> {
        use std::os::unix::io::AsFd as _;
        let fd = stdin.as_fd();
        let orig = nix::sys::termios::tcgetattr(fd).ok()?;
        let mut raw = orig.clone();
        raw.local_flags.remove(nix::sys::termios::LocalFlags::ECHO);
        nix::sys::termios::tcsetattr(fd, nix::sys::termios::SetArg::TCSANOW, &raw).ok()?;
        // Ctrl+C during the read must abort via EINTR (restoring echo in
        // Drop), not kill with echo left off. No SA_RESTART on purpose.
        let act = nix::sys::signal::SigAction::new(
            nix::sys::signal::SigHandler::Handler(eintr_abort),
            nix::sys::signal::SaFlags::empty(),
            nix::sys::signal::SigSet::empty(),
        );
        let old_sigint =
            unsafe { nix::sys::signal::sigaction(nix::sys::signal::Signal::SIGINT, &act).ok()? };
        Some(NoEcho {
            fd,
            orig,
            old_sigint,
        })
    }
}

/// Empty on purpose: its existence (without SA_RESTART) turns Ctrl+C
/// into EINTR on the blocked stdin read, which read_secret maps to
/// failure so the guard drops, echo restores, and we exit cleanly.
#[cfg(unix)]
extern "C" fn eintr_abort(_: nix::libc::c_int) {}

#[cfg(unix)]
impl Drop for NoEcho<'_> {
    fn drop(&mut self) {
        let _ =
            nix::sys::termios::tcsetattr(self.fd, nix::sys::termios::SetArg::TCSANOW, &self.orig);
        unsafe {
            let _ = nix::sys::signal::sigaction(nix::sys::signal::Signal::SIGINT, &self.old_sigint);
        }
    }
}

fn auth_rm(provider: &str) {
    match SystemKeychain.delete(provider) {
        Ok(()) => println!("{provider}: deleted"),
        Err(e) => {
            eprintln!("{provider}: not deleted: {e}");
            std::process::exit(1);
        }
    }
}

fn blocks(argv: &[&str]) {
    let mut db: Option<String> = None;
    let mut limit: usize = 20;
    let mut session: Option<String> = None;
    let mut i = 0;
    while i < argv.len() {
        match argv[i] {
            "--db" => {
                i += 1;
                db = match argv.get(i) {
                    Some(v) => Some(v.to_string()),
                    None => {
                        eprintln!("--db needs a value");
                        std::process::exit(2);
                    }
                };
            }
            "--limit" => {
                i += 1;
                limit = match argv.get(i) {
                    Some(v) => v.parse().unwrap_or_else(|_| {
                        eprintln!("--limit needs a number");
                        std::process::exit(2);
                    }),
                    None => {
                        eprintln!("--limit needs a value");
                        std::process::exit(2);
                    }
                };
            }
            "--session" => {
                i += 1;
                session = match argv.get(i) {
                    Some(v) => Some(v.to_string()),
                    None => {
                        eprintln!("--session needs a value");
                        std::process::exit(2);
                    }
                };
            }
            other => {
                eprintln!("unknown blocks flag {other:?}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    let store = match db {
        Some(path) => zet_blocks::Store::open(&path).unwrap_or_else(|e| {
            eprintln!("cannot open {path}: {e}");
            std::process::exit(1);
        }),
        None => zet_blocks::Store::open_default().unwrap_or_else(|e| {
            eprintln!("cannot open block store: {e}");
            std::process::exit(1);
        }),
    };
    let list = match &session {
        Some(s) => store.list(s).unwrap_or_else(|e| {
            eprintln!("cannot list blocks: {e}");
            std::process::exit(1);
        }),
        None => store.recent(limit).unwrap_or_else(|e| {
            eprintln!("cannot list blocks: {e}");
            std::process::exit(1);
        }),
    };
    let list = if session.is_some() {
        list.into_iter().rev().take(limit).rev().collect::<Vec<_>>()
    } else {
        list
    };
    for b in list {
        println!(
            "#{} [exit {}] ({}) {}",
            b.id, b.exit_code, b.session, b.command
        );
        for line in b.output.lines() {
            println!("  {line}");
        }
    }
}

fn journal(argv: &[&str]) {
    let mut limit = 20;
    let mut i = 0;
    while i < argv.len() {
        match argv[i] {
            "--limit" => {
                i += 1;
                limit = match argv.get(i) {
                    Some(v) => v.parse().unwrap_or_else(|_| {
                        eprintln!("--limit needs a number");
                        std::process::exit(2);
                    }),
                    None => {
                        eprintln!("--limit needs a value");
                        std::process::exit(2);
                    }
                };
            }
            other => {
                eprintln!("unknown journal flag {other:?}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    let entries = zet_agent::read_journal(&zet_agent::journal_path_default()).unwrap_or_else(|e| {
        eprintln!("cannot read journal: {e}");
        std::process::exit(1);
    });
    if entries.is_empty() {
        println!("no agent turns recorded");
        return;
    }
    let start = entries.len().saturating_sub(limit);
    for (k, e) in entries[start..].iter().enumerate() {
        let verdict = if !e.approved {
            "denied".to_string()
        } else if e.executed {
            format!(
                "exit {}",
                e.exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".to_string())
            )
        } else {
            "not run".to_string()
        };
        println!("#{} [{:?} {}] {}", start + k, e.tier, verdict, e.command);
        let shown: String = e.output.chars().take(800).collect();
        for line in shown.lines() {
            println!("  {line}");
        }
        if e.output.chars().count() > 800 {
            println!("  … ({} chars total)", e.output.chars().count());
        }
    }
}
