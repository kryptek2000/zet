use zet_models::keys::{
    opencode_auth_path, read_opencode_key_at, EnvKeychain, Keychain, SystemKeychain,
};

const PROVIDERS: &[&str] = &["openai", "openrouter", "compat", "opencode"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match argv.as_slice() {
        ["auth", "show"] => auth_show(),
        ["auth", "add", provider] => auth_add(provider),
        ["auth", "rm", provider] => auth_rm(provider),
        ["inline"] => inline_show(),
        ["inline", "off"] => inline_clear(),
        ["inline", spec] => inline_set(spec),
        ["models", rest @ ..] => models_cmd(rest),
        ["blocks", rest @ ..] => blocks(rest),
        ["journal", rest @ ..] => journal(rest),
        ["undo"] => undo(),
        ["run", rest @ ..] => run_cmd(rest),
        ["runs", rest @ ..] => runs(rest),
        ["__run_worker", id] => run_worker(id),
        // Known group, bad usage: hint the group, not the top level.
        // (Guarded by the shared list so dispatch and the window router
        // can't drift apart — see zet_pty::CLI_SUBCOMMANDS.)
        [sub, ..] if zet_pty::CLI_SUBCOMMANDS.contains(sub) => {
            eprintln!("bad usage for `zet {sub}`");
            std::process::exit(2);
        }
        _ => {
            eprintln!("usage:");
            eprintln!("  zet auth show             list providers and key status");
            eprintln!("  zet auth add <provider>   prompt for key, store in OS keychain");
            eprintln!("  zet auth rm <provider>    delete key from OS keychain");
            eprintln!("  zet inline [SPEC|off]     show/set/clear the NL/agent model");
            eprintln!("                            (e.g. openrouter:openai/gpt-4o-mini, local:llama3.2:1b)");
            eprintln!("  zet models [--free] [URL] list choosable models (ready specs)");
            eprintln!("                            (openrouter / opencode-go keys, local server, compat URL)");
            eprintln!("  zet blocks [--db PATH] [--limit N] [--session S]");
            eprintln!("                            list recorded command blocks");
            eprintln!("  zet journal [--limit N]   agent turns + recent commands (one history)");
            eprintln!("  zet undo                  undo the last checkpointed agent step here");
            eprintln!("  zet run <goal...>         run an agent turn here, unattended (never T3)");
            eprintln!("  zet run --background <g>  delegate it (log + bubble when done)");
            eprintln!("  zet runs                  list delegated runs");
            std::process::exit(2);
        }
    }
}

fn auth_show() {
    for provider in PROVIDERS {
        let status = if *provider == "opencode" {
            // Go login lives with the CLI, not (only) the keychain.
            if read_opencode_key_at(&opencode_auth_path()).is_some() {
                "set (opencode cli)"
            } else if EnvKeychain.get(provider).is_some() {
                "set (env)"
            } else if SystemKeychain.get(provider).is_some() {
                "set (keychain)"
            } else {
                "missing (`opencode auth login opencode`)"
            }
        } else if EnvKeychain.get(provider).is_some() {
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

fn inline_show() {
    let path = zet_models::keys::inline_spec_path();
    if let Ok(spec) = std::env::var("ZET_INLINE_SPEC") {
        println!("{spec} (env)");
    } else if let Some(spec) = zet_models::keys::read_inline_spec_at(&path) {
        println!("{spec} (set via `zet inline`)");
    } else if let Some(spec) = zet_models::keys::autodetect_local() {
        println!("{spec} (autodetected local)");
    } else {
        println!("unset (no ZET_INLINE_SPEC, no `zet inline` spec, no local model)");
    }
}

fn inline_set(spec: &str) {
    // Shape provider:model — a bare model name is meaningless without one.
    if !spec.contains(':') {
        eprintln!("bad spec {spec:?}: want PROVIDER:MODEL, e.g. openrouter:openai/gpt-4o-mini");
        std::process::exit(2);
    }
    let path = zet_models::keys::inline_spec_path();
    match zet_models::keys::write_inline_spec_at(&path, spec) {
        Ok(()) => println!("inline model: {spec} (windows pick it up, no restart)"),
        Err(e) => {
            eprintln!("failed to write {}: {e}", path.display());
            std::process::exit(1);
        }
    }
}

fn inline_clear() {
    let path = zet_models::keys::inline_spec_path();
    match zet_models::keys::clear_inline_spec_at(&path) {
        Ok(()) => println!("inline model cleared (falls back to env, then local autodetect)"),
        Err(e) => {
            eprintln!("failed to clear {}: {e}", path.display());
            std::process::exit(1);
        }
    }
}

/// List choosable models as ready `zet inline` specs: OpenRouter (key),
/// local server (no key), or one compat URL. `--free` keeps `:free` ids.
fn models_cmd(argv: &[&str]) {
    let mut free_only = false;
    let mut url: Option<&str> = None;
    for a in argv {
        if *a == "--free" {
            free_only = true;
        } else if a.starts_with('-') {
            eprintln!("unknown flag {a:?}: want --free");
            std::process::exit(2);
        } else if url.is_none() {
            url = Some(a);
        } else {
            eprintln!("too many args: want [--free] [URL]");
            std::process::exit(2);
        }
    }
    let keep = |id: &str| !free_only || zet_models::is_free_model(id);
    let mut listed = 0;
    if let Some(url) = url {
        // One compat endpoint, fail loud (explicitly asked).
        let key = keychains(|c| zet_models::keys::resolve_key("compat", c));
        let Some(key) = key else {
            eprintln!("no key for 'compat': run `zet auth add compat` or set ZET_COMPAT_API_KEY");
            std::process::exit(1);
        };
        match zet_models::list_models(url, &key) {
            Ok(ids) => {
                for id in ids.iter().filter(|id| keep(id)) {
                    println!("compat:{url}:{id}");
                    listed += 1;
                }
            }
            Err(e) => {
                eprintln!("{url}: {e}");
                std::process::exit(1);
            }
        }
    } else {
        // Sweep every source with credentials; misses note to stderr.
        let or_key = keychains(|c| zet_models::keys::resolve_key("openrouter", c));
        match or_key {
            Some(key) => match zet_models::list_models("https://openrouter.ai/api/v1", &key) {
                Ok(ids) => {
                    for id in ids.iter().filter(|id| keep(id)) {
                        println!("openrouter:{id}");
                        listed += 1;
                    }
                }
                Err(e) => eprintln!("openrouter: {e}"),
            },
            None => eprintln!("openrouter: skipped (no key — `zet auth add openrouter`)"),
        }
        let go_key = read_opencode_key_at(&opencode_auth_path())
            .or_else(|| keychains(|c| zet_models::keys::resolve_key("opencode", c)));
        match go_key {
            Some(key) => match zet_models::list_models("https://opencode.ai/zen/go/v1", &key) {
                Ok(ids) => {
                    for id in ids.iter().filter(|id| keep(id)) {
                        println!("opencode:{id}");
                        listed += 1;
                    }
                }
                Err(e) => eprintln!("opencode: {e}"),
            },
            None => eprintln!("opencode: skipped (`opencode auth login opencode`)"),
        }
        match zet_models::keys::local_tags_at("127.0.0.1", 11434) {
            Some(names) => {
                for name in names.iter().filter(|n| keep(n)) {
                    println!("local:{name}");
                    listed += 1;
                }
            }
            None => eprintln!("local: skipped (no server on 127.0.0.1:11434)"),
        }
    }
    if listed == 0 {
        eprintln!("no models listed");
        std::process::exit(1);
    }
}

/// Keychain search order for CLI reads (env first, OS keychain second).
fn keychains<T>(f: impl FnOnce(&[&dyn Keychain]) -> T) -> T {
    let env = EnvKeychain;
    let sys = SystemKeychain;
    let chains: [&dyn Keychain; 2] = [&env, &sys];
    f(&chains)
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

/// One agent turn in this directory, journaled. Shared by foreground
/// `zet run` and the background worker.
fn execute_turn(goal: &str) -> Result<String, String> {
    let path = zet_models::keys::inline_spec_path();
    let spec = match std::env::var("ZET_INLINE_SPEC") {
        Ok(s) => s,
        Err(_) => match zet_models::keys::read_inline_spec_at(&path) {
            Some(s) => s,
            None => match zet_models::keys::autodetect_local() {
                Some(s) => s,
                None => {
                    return Err("no model: set ZET_INLINE_SPEC or `zet inline <spec>`".to_string());
                }
            },
        },
    };
    let provider =
        keychains(|c| zet_models::keys::resolve_provider(&spec, c)).map_err(|e| e.to_string())?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let in_repo = zet_agent::detect_repo(&cwd);
    let git = zet_context::git_state(&cwd);
    let context = zet_context::gather(
        &[],
        &cwd.to_string_lossy(),
        git,
        zet_context::DEFAULT_BUDGET,
    )
    .render();
    let mut agent =
        zet_agent::Agent::new(provider.into_client(), zet_agent::Unattended, in_repo, 8)
            .with_workdir(cwd)
            .with_context(context);
    let summary = agent.run(goal).map_err(|e| e.to_string())?;
    let _ = zet_agent::append_journal(&zet_agent::journal_path_default(), &agent.journal);
    Ok(summary)
}

/// Delegated run: an agent turn in this directory, unattended. T3 is
/// never approved (no human to type-confirm); the turn is journaled
/// so `zet journal` shows it and `zet undo` reaches it.
/// `--background` detaches: the turn outlives this shell, the summary
/// lands in the run log + journal, and an OS bubble announces it.
fn run_cmd(argv: &[&str]) {
    let background = argv
        .first()
        .is_some_and(|a| *a == "--background" || *a == "--bg" || *a == "-b");
    let argv = if background { &argv[1..] } else { argv };
    if argv.is_empty() {
        eprintln!("usage: zet run [--background] <goal...>");
        std::process::exit(2);
    }
    if argv.iter().any(|a| a.starts_with('-')) {
        eprintln!("zet run takes a goal, not flags");
        std::process::exit(2);
    }
    let goal = argv.join(" ");
    if background {
        delegate(&goal);
        return;
    }
    match execute_turn(&goal) {
        Ok(summary) => println!("{summary}"),
        Err(e) => {
            eprintln!("agent error: {e}");
            std::process::exit(1);
        }
    }
}

/// Runs directory: one subdir per delegated turn (goal, log, status).
fn runs_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::Path::new(&home).join(".zet").join("runs")
}

/// Detach a worker for the goal; print the run id and exit now. The
/// worker survives shell exit (own session); output streams to its log.
fn delegate(goal: &str) {
    let id = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        std::process::id()
    );
    let dir = runs_dir().join(&id);
    if std::fs::create_dir_all(&dir).is_err() {
        eprintln!("cannot create run dir");
        std::process::exit(1);
    }
    if std::fs::write(dir.join("goal.txt"), goal).is_err() {
        eprintln!("cannot write run goal");
        std::process::exit(1);
    }
    let _ = std::fs::write(dir.join("status"), "running");
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("cannot find zet binary: {e}");
            std::process::exit(1);
        }
    };
    let log = std::fs::File::create(dir.join("log.txt")).unwrap_or_else(|e| {
        eprintln!("cannot create run log: {e}");
        std::process::exit(1);
    });
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        let log_err = log
            .try_clone()
            .unwrap_or_else(|_| std::fs::File::create("/dev/null").expect("devnull"));
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("__run_worker")
            .arg(&id)
            .stdin(std::process::Stdio::null())
            .stdout(log)
            .stderr(log_err);
        // Own session: outlives the delegating shell and its terminal.
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid()
                    .map(|_| ())
                    .map_err(std::io::Error::other)
            });
        }
        if cmd.spawn().is_err() {
            eprintln!("cannot spawn worker");
            std::process::exit(1);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (exe, log);
        eprintln!("background runs need unix");
        std::process::exit(1);
    }
    println!("delegated {id} (log: {})", dir.join("log.txt").display());
}

/// Background worker: run the recorded goal, then log + journal +
/// notify. Never touches the terminal that delegated it.
fn run_worker(id: &str) {
    let dir = runs_dir().join(id);
    let goal = std::fs::read_to_string(dir.join("goal.txt")).unwrap_or_default();
    let summary = match execute_turn(goal.trim()) {
        Ok(s) => s,
        Err(e) => format!("agent error: {e}"),
    };
    let _ = std::fs::write(dir.join("log.txt"), &summary);
    let _ = std::fs::write(dir.join("status"), "done");
    let first: String = summary
        .lines()
        .next()
        .unwrap_or("done")
        .chars()
        .take(120)
        .collect();
    let _ = notify_rust::Notification::new()
        .summary("ZET run done")
        .body(&format!("{id}: {first}"))
        .show();
}

/// List delegated runs (oldest first): id, status, and goal.
fn runs(argv: &[&str]) {
    if argv.iter().any(|a| a.starts_with('-')) {
        eprintln!("zet runs takes no flags");
        std::process::exit(2);
    }
    let dir = runs_dir();
    let mut ids: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| {
                e.ok().and_then(|e| {
                    if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        e.file_name().into_string().ok()
                    } else {
                        None
                    }
                })
            })
            .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        println!("no background runs");
        return;
    }
    ids.sort();
    for id in ids {
        let sub = dir.join(&id);
        let status =
            std::fs::read_to_string(sub.join("status")).unwrap_or_else(|_| "?".to_string());
        let goal = std::fs::read_to_string(sub.join("goal.txt")).unwrap_or_default();
        let goal: String = goal.chars().take(80).collect();
        println!("{} [{}] {}", id, status.trim(), goal.trim());
    }
}
/// One-key undo behind `zet undo`: the last checkpointed agent step,
/// applied to the current directory. The window's F12 is the same call
/// on the live shell — this is its backdoor when the key never arrives.
fn undo() {
    let entries = zet_agent::read_journal(&zet_agent::journal_path_default()).unwrap_or_else(|e| {
        eprintln!("cannot read journal: {e}");
        std::process::exit(1);
    });
    let Some(cp) = entries.iter().rev().find_map(|e| e.checkpoint.clone()) else {
        println!("nothing to undo");
        return;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match zet_agent::undo_checkpoint(&cwd, &Some(cp)) {
        Ok(()) => println!("undone"),
        Err(e) => {
            eprintln!("undo failed: {e}");
            std::process::exit(1);
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
    } else {
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
    // One history: turns above, the commands behind them below. The
    // block store owns open errors (`zet blocks` reports them); here a
    // missing store just means no commands to show.
    if let Ok(store) = zet_blocks::Store::open_default() {
        if let Ok(list) = store.recent(limit) {
            if !list.is_empty() {
                println!("recent commands:");
                for b in &list {
                    println!("#{} [exit {}] {}", b.id, b.exit_code, b.command);
                }
            }
        }
    }
}
