use zet_models::keys::{
    opencode_auth_path, read_opencode_key_at, EnvKeychain, Keychain, SystemKeychain,
};

const PROVIDERS: &[&str] = &["openai", "openrouter", "compat", "opencode"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = zet::mode::Env::detect();
    match zet::mode::plan(&args, &env) {
        zet::mode::Mode::Window { goal } => match zet::window::run(goal) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("zet: {e}");
                std::process::exit(1);
            }
        },
        zet::mode::Mode::Cli => cli_dispatch(&args),
        // A window was asked for but this box cannot open one (no
        // display, or ZET_NO_WINDOW): say so plainly, then the usage.
        zet::mode::Mode::Usage => {
            if zet::mode::window_refused(&args, &env) {
                eprintln!("zet: no display; run a subcommand (zet auth|blocks|run|...)");
            }
            cli_dispatch(&args);
        }
    }
}

fn cli_dispatch(args: &[String]) {
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
        ["replay", rest @ ..] => replay(rest),
        ["journal", rest @ ..] => journal(rest),
        ["undo"] => undo(),
        ["run", rest @ ..] => run_cmd(rest),
        ["runs", rest @ ..] => runs(rest),
        ["usage"] => usage(),
        ["schedules"] => schedules(),
        ["unschedule", id] => unschedule(id),
        ["daemon"] => daemon(),
        ["daemon", flag] if flag == &"--install" || flag == &"--uninstall" => {
            daemon_service(flag == &"--install")
        }
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
            eprintln!("                            (--session defaults to $ZET_SESSION)");
            eprintln!("  zet replay [--db PATH] <session>");
            eprintln!("                            print a session's transcript, block by block");
            eprintln!("                            (session defaults to $ZET_SESSION)");
            eprintln!("  zet journal [--limit N]   agent turns + recent commands (one history)");
            eprintln!("  zet undo                  undo the last checkpointed agent step here");
            eprintln!("  zet run <goal...>         run an agent turn here, unattended (never T3)");
            eprintln!("  zet run --background <g>  delegate it (log + bubble when done)");
            eprintln!("  zet runs                  list delegated runs");
            eprintln!("  zet usage                 estimated model tokens (chars/4)");
            eprintln!("  zet run --every 1h <g>    repeat on schedule (daemon fires)");
            eprintln!("  zet schedules             list repeating goals");
            eprintln!("  zet unschedule <id>       drop a repeating goal");
            eprintln!("  zet daemon                run due schedules (Ctrl+C stops)");
            eprintln!("  zet daemon --install     autostart on login (systemd user unit)");
            eprintln!("  zet daemon --uninstall   remove autostart");
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

/// The session the window exported into this shell (`ZET_SESSION`).
/// None when the variable is unset or empty: a command run outside a
/// zet window names no session, and guessing one would filter the
/// block list down to nothing.
fn env_session() -> Option<String> {
    std::env::var("ZET_SESSION").ok().filter(|s| !s.is_empty())
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
    // No --session: fall back to the session this shell inherited from
    // the window. Inside a zet window that is "my blocks"; outside one
    // the variable is unset and the list spans every session.
    let session = session.or_else(env_session);
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
        // The branch rides along when the recorder knew one; a command
        // from outside a repo shows no tag at all rather than `[]`.
        let branch = if b.git_branch.is_empty() {
            String::new()
        } else {
            format!("[{}] ", b.git_branch)
        };
        println!(
            "#{} [exit {}] ({}) {}{}",
            b.id, b.exit_code, b.session, branch, b.command
        );
        for line in b.output.lines() {
            println!("  {line}");
        }
    }
}

/// `zet replay [--db PATH] <session>`: print one session's transcript.
/// Arg parsing and exits stay at this edge so the transcript itself is
/// testable in-process.
fn replay(argv: &[&str]) {
    let mut db: Option<String> = None;
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
            // A flag we don't know is a usage error; anything else is
            // the session name.
            other if other.starts_with('-') => {
                eprintln!("unknown replay flag {other:?}");
                std::process::exit(2);
            }
            other => {
                if session.is_some() {
                    eprintln!("replay takes one session, got two: {session:?} and {other:?}");
                    std::process::exit(2);
                }
                session = Some(other.to_string());
            }
        }
        i += 1;
    }
    // No session given: the window's session id, when this shell was
    // started under one. Without it there is nothing to replay and the
    // usage stands.
    let session = session.or_else(env_session);
    let Some(session) = session else {
        eprintln!("usage: zet replay [--db PATH] <session>");
        std::process::exit(2);
    };
    match replay_cmd(db.as_deref(), &session) {
        Ok(text) => print!("{text}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

/// The transcript for `session`, byte-identical to what the blocks were
/// recorded as (`$ cmd`, output, `[exit N, Nms]` per block). An unknown
/// session is an error: a silent empty transcript reads like "this
/// session did nothing", which is a different claim and a wrong one.
fn replay_cmd(db: Option<&str>, session: &str) -> Result<String, String> {
    let store = match db {
        Some(path) => {
            zet_blocks::Store::open(path).map_err(|e| format!("cannot open {path}: {e}"))?
        }
        None => zet_blocks::Store::open_default()
            .map_err(|e| format!("cannot open block store: {e}"))?,
    };
    let text = store
        .replay(session)
        .map_err(|e| format!("cannot replay session: {e}"))?;
    if text.is_empty() {
        return Err(format!("no blocks for session {session:?}"));
    }
    Ok(text)
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
    // Optional repeat: --every 1h (s/m/h/d suffixes, bare seconds).
    let mut every_secs: Option<u64> = None;
    let mut argv = argv;
    if argv.first().is_some_and(|a| *a == "--every") {
        let dur = argv.get(1).copied().unwrap_or("");
        every_secs = Some(parse_duration(dur).unwrap_or_else(|| {
            eprintln!("bad --every {dur:?}: want like 30m, 1h, 1d, 3600");
            std::process::exit(2);
        }));
        argv = &argv[2.min(argv.len())..];
    }
    // --arg k=v pairs fill recipe templates; everything else is goal.
    let mut args = Vec::new();
    let mut kv: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == "--arg" {
            i += 1;
            match argv.get(i) {
                Some(pair) => match pair.split_once('=') {
                    Some((k, v)) => kv.push((k.to_string(), v.to_string())),
                    None => {
                        eprintln!("--arg wants k=v, got {pair:?}");
                        std::process::exit(2);
                    }
                },
                None => {
                    eprintln!("--arg needs k=v");
                    std::process::exit(2);
                }
            }
        } else {
            args.push(argv[i]);
        }
        i += 1;
    }
    if args.is_empty() {
        eprintln!("usage: zet run [--background] [--arg k=v ...] <goal... | recipe:name ...>");
        std::process::exit(2);
    }
    if args.iter().any(|a| a.starts_with('-')) {
        eprintln!("zet run takes a goal, not flags");
        std::process::exit(2);
    }
    let mut goal = args.join(" ");
    if let Some(name) = args.first().and_then(|w| w.strip_prefix("recipe:")) {
        let extra = args[1..].join(" ");
        goal = render_recipe(name, &extra, &kv);
    }
    // Repeating goal: record the schedule, then run now like a normal
    // run (foreground or delegated). The daemon fires repeats.
    if let Some(secs) = every_secs {
        schedule_add(&goal, secs);
    }
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

/// Named goal templates (`~/.zet/recipes/<name>.md`) with {{arg}}
/// placeholders: `zet run recipe:triage --arg service=api`.
/// Missing recipes list what's there; unfilled placeholders fail loud.
fn render_recipe(name: &str, extra: &str, args: &[(String, String)]) -> String {
    // Jail the name: dir.join with an absolute or ../ name escapes the
    // recipes dir into arbitrary *.md reads. Fail loud, not canonical.
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        eprintln!("bad recipe name {name:?}: plain names only");
        std::process::exit(1);
    }
    let dir = zet_blocks::zet_home().join("recipes");
    let path = dir.join(format!("{name}.md"));
    let body = match std::fs::read_to_string(&path) {
        Ok(b) => b,
        Err(_) => {
            let mut available: Vec<String> = std::fs::read_dir(&dir)
                .map(|rd| {
                    rd.filter_map(|e| {
                        e.ok().and_then(|e| {
                            e.path().extension().is_some_and(|x| x == "md").then(|| {
                                e.path()
                                    .file_stem()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned()
                            })
                        })
                    })
                    .collect()
                })
                .unwrap_or_default();
            available.sort();
            if available.is_empty() {
                eprintln!("unknown recipe {name:?} (no recipes in {})", dir.display());
            } else {
                eprintln!("unknown recipe {name:?}; have: {}", available.join(", "));
            }
            if name.contains(char::is_whitespace) {
                eprintln!("hint: pass recipe and --arg as separate words, no quotes:");
                eprintln!("  zet run recipe:greet --arg name=world");
            }
            std::process::exit(1);
        }
    };
    let mut goal = body;
    for (k, v) in args {
        goal = goal.replace(&format!("{{{{{k}}}}}"), v);
    }
    // Unfilled placeholders fail loud (a silent {{name}} would confuse
    // the model into asking or guessing). Unterminated {{ fails too.
    let mut missing = Vec::new();
    let mut rest = goal.as_str();
    let mut unterminated = false;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            missing.push(after[..end].to_string());
            rest = &after[end + 2..];
        } else {
            unterminated = true;
            break;
        }
    }
    if unterminated {
        eprintln!("recipe {name:?} has unterminated {{{{...}}}} placeholder");
        std::process::exit(1);
    }
    if !missing.is_empty() {
        missing.sort();
        missing.dedup();
        let want: Vec<String> = missing.iter().map(|m| format!("--arg {m}=...")).collect();
        eprintln!("recipe {name:?} needs {}", want.join(" "));
        std::process::exit(1);
    }
    if extra.trim().is_empty() {
        goal.trim().to_string()
    } else {
        format!("{}\n{}", goal.trim(), extra.trim())
    }
}

/// Durations for --every: bare seconds or s/m/h/d suffixes.
fn parse_duration(text: &str) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    let (num, mult) = match text.chars().last() {
        Some('s') => (&text[..text.len() - 1], 1),
        Some('m') => (&text[..text.len() - 1], 60),
        Some('h') => (&text[..text.len() - 1], 3600),
        Some('d') => (&text[..text.len() - 1], 86400),
        Some(c) if c.is_ascii_digit() => (text, 1),
        _ => return None,
    };
    let n: u64 = num.parse().ok()?;
    if n == 0 {
        return None;
    }
    n.checked_mul(mult)
}

/// Schedules file: one JSON object per line (no locking — single
/// writer per command invocation; the daemon rewrites whole).
fn schedules_path() -> std::path::PathBuf {
    zet_blocks::zet_home().join("schedules.jsonl")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Record a repeating goal; first repeat fires one interval out (this
/// run already covers now).
fn schedule_add(goal: &str, every_secs: u64) {
    let id = format!("sch-{}-{}", now_secs(), std::process::id());
    let line = serde_json::json!({
        "id": id,
        "goal": goal,
        "every_secs": every_secs,
        "last_run": now_secs(),
    });
    let path = schedules_path();
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            eprintln!("cannot create schedules dir");
            std::process::exit(1);
        }
    }
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|e| {
            eprintln!("cannot write schedule: {e}");
            std::process::exit(1);
        });
    if writeln!(f, "{line}").is_err() {
        eprintln!("cannot write schedule");
        std::process::exit(1);
    }
    println!("scheduled {id} every {every_secs}s");
}

/// List schedules: id, interval, goal.
fn schedules() {
    let text = std::fs::read_to_string(schedules_path()).unwrap_or_default();
    let mut rows = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let (Some(id), Some(goal), Some(every)) = (
            v["id"].as_str(),
            v["goal"].as_str(),
            v["every_secs"].as_u64(),
        ) else {
            continue;
        };
        rows.push((id.to_string(), every, goal.to_string()));
    }
    if rows.is_empty() {
        println!("no schedules");
        return;
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, every, goal) in rows {
        let goal: String = goal.chars().take(80).collect();
        println!("{id} every {every}s: {}", goal.trim());
    }
}

/// Remove the schedule with the exact id (or every id sharing the
/// prefix — all matches are dropped, each printed, so a short prefix
/// is loud about what it killed).
fn unschedule(prefix: &str) {
    let path = schedules_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v["id"].as_str() {
            Some(id) if id.starts_with(prefix) => dropped.push(id.to_string()),
            _ => kept.push(line.to_string()),
        }
    }
    if dropped.is_empty() {
        eprintln!("no schedule matching {prefix:?}");
        std::process::exit(1);
    }
    if std::fs::write(
        &path,
        kept.join("\n") + if kept.is_empty() { "" } else { "\n" },
    )
    .is_err()
    {
        eprintln!("cannot update schedules");
        std::process::exit(1);
    }
    for id in dropped {
        println!("unscheduled {id}");
    }
}

/// Foreground scheduler loop: every 60s, delegate due goals as
/// background runs (log + bubble on done). Ctrl+C stops it.
fn daemon() {
    println!("zet daemon: watching schedules (Ctrl+C stops)");
    loop {
        daemon_tick();
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

/// Autostart: a systemd user unit that runs `zet daemon` on login.
/// --install writes the unit (ExecStart = this binary) and enables it;
/// --uninstall disables and removes it. systemctl failures are loud
/// notes, not silent: the file is the install, activation is reported.
fn daemon_service(install: bool) {
    let config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.config")
    });
    let unit = std::path::Path::new(&config)
        .join("systemd")
        .join("user")
        .join("zet-daemon.service");
    if !install {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "disable", "--now", "zet-daemon.service"])
            .output();
        if std::fs::remove_file(&unit).is_ok() {
            println!("removed {}", unit.display());
        } else {
            eprintln!("no autostart installed at {}", unit.display());
            std::process::exit(1);
        }
        return;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| "zet".into());
    let body = format!(
        "[Unit]\nDescription=ZET scheduled runs daemon\nAfter=network-online.target\n\n\
         [Service]\nExecStart={} daemon\nRestart=on-failure\nRestartSec=60\n\n\
         [Install]\nWantedBy=default.target\n",
        exe.display()
    );
    if let Some(parent) = unit.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            eprintln!("cannot create {}", parent.display());
            std::process::exit(1);
        }
    }
    if std::fs::write(&unit, &body).is_err() {
        eprintln!("cannot write {}", unit.display());
        std::process::exit(1);
    }
    println!("wrote {}", unit.display());
    for step in [
        vec!["--user", "daemon-reload"],
        vec!["--user", "enable", "--now", "zet-daemon.service"],
    ] {
        match std::process::Command::new("systemctl").args(&step).output() {
            Ok(o) if o.status.success() => {}
            _ => {
                eprintln!("systemctl {} failed — enable manually:", step.join(" "));
                eprintln!("  systemctl --user daemon-reload");
                eprintln!("  systemctl --user enable --now zet-daemon.service");
                return;
            }
        }
    }
    println!("zet daemon autostart enabled");
}

fn daemon_tick() {
    let path = schedules_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let now = now_secs();
    let mut changed = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(mut v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let (Some(goal), Some(every), Some(last)) = (
            v["goal"].as_str().map(str::to_string),
            v["every_secs"].as_u64(),
            v["last_run"].as_u64(),
        ) else {
            out.push(line.to_string());
            continue;
        };
        if every > 0 && now.saturating_sub(last) >= every {
            delegate(&goal);
            v["last_run"] = serde_json::Value::from(now);
            changed = true;
        }
        out.push(v.to_string());
    }
    if changed {
        let _ = std::fs::write(&path, out.join("\n") + "\n");
    }
}
/// Runs directory: one subdir per delegated turn (goal, log, status).
fn runs_dir() -> std::path::PathBuf {
    zet_blocks::zet_home().join("runs")
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

/// Estimated model usage from ~/.zet/usage.jsonl: calls, tokens
/// (chars/4 heuristic, clearly labeled), latency, per-model split.
fn usage() {
    let path = zet_models::metered::default_log();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            println!("no usage recorded yet");
            return;
        }
    };
    let mut calls = 0u64;
    let (mut ink, mut outk) = (0u64, 0u64);
    let (mut ms_total, mut ms_n) = (0u64, 0u64);
    let mut per_label: std::collections::HashMap<String, (u64, u64, u64)> =
        std::collections::HashMap::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let (Some(i), Some(o)) = (v["in_est"].as_u64(), v["out_est"].as_u64()) else {
            continue;
        };
        calls += 1;
        ink += i;
        outk += o;
        if let Some(ms) = v["ms"].as_u64() {
            ms_total += ms;
            ms_n += 1;
        }
        let label = v["label"].as_str().unwrap_or("?").to_string();
        let e = per_label.entry(label).or_insert((0, 0, 0));
        e.0 += 1;
        e.1 += i;
        e.2 += o;
    }
    if calls == 0 {
        println!("no usage recorded yet");
        return;
    }
    println!("{calls} calls, ~{ink} in / ~{outk} out (estimated tokens, chars/4)");
    if ms_n > 0 {
        println!("avg latency: {}ms", ms_total.checked_div(ms_n).unwrap_or(0));
    }
    let mut labels: Vec<_> = per_label.into_iter().collect();
    labels.sort_by_key(|(_, (_, i, o))| std::cmp::Reverse(i + o));
    for (label, (n, i, o)) in labels {
        println!("  {label}: {n} calls, ~{i} in / ~{o} out");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "zet_replay-{tag}-{}-{nanos}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn block(
        session: &str,
        command: &str,
        output: &str,
        exit_code: i32,
        ms: u64,
    ) -> zet_blocks::Block {
        zet_blocks::Block {
            id: 0,
            session: session.to_string(),
            command: command.to_string(),
            output: output.to_string(),
            exit_code,
            cwd: "/tmp".to_string(),
            git_branch: "main".to_string(),
            duration_ms: ms,
        }
    }

    /// zet#4 acceptance: `zet replay` reproduces the recorded output,
    /// byte for byte — nonzero exit, timings, and an unterminated last
    /// line included.
    #[test]
    fn replay_reproduces_the_transcript_byte_for_byte() {
        let path = temp_db("accept");
        let store = zet_blocks::Store::open(path.to_str().unwrap()).unwrap();
        store
            .append(&block("s1", "echo alpha", "alpha\n", 0, 3))
            .unwrap();
        store
            .append(&block("s1", "boom", "kaboom\n", 1, 12))
            .unwrap();
        // Unterminated output: the marker must not run into the text.
        store.append(&block("s1", "printf hi", "hi", 0, 1)).unwrap();
        drop(store);

        let out = replay_cmd(Some(path.to_str().unwrap()), "s1").unwrap();
        assert_eq!(
            out,
            "$ echo alpha\nalpha\n[exit 0, 3ms]\n\
             $ boom\nkaboom\n[exit 1, 12ms]\n\
             $ printf hi\nhi\n[exit 0, 1ms]\n"
        );
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn replay_of_an_unknown_session_fails_loudly() {
        let path = temp_db("unknown");
        let store = zet_blocks::Store::open(path.to_str().unwrap()).unwrap();
        store
            .append(&block("s1", "echo alpha", "alpha\n", 0, 3))
            .unwrap();
        drop(store);

        let err = replay_cmd(Some(path.to_str().unwrap()), "nope").unwrap_err();
        assert!(err.contains("no blocks for session"), "{err}");
        assert!(err.contains("nope"), "{err}");
        // A session that exists but holds nothing reads the same way:
        // silence would claim the session did nothing.
        let empty = zet_blocks::Store::open_in_memory().unwrap();
        assert!(empty.replay("s1").unwrap().is_empty());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn replay_reports_a_store_it_cannot_open() {
        let err = replay_cmd(Some("/nonexistent/blocks.sqlite"), "s1").unwrap_err();
        assert!(err.starts_with("cannot open"), "{err}");
    }

    /// The transcript is the recorded bytes: the branch is metadata, and
    /// showing it in the replay would change the format under replay.
    #[test]
    fn the_branch_never_leaks_into_the_transcript() {
        let path = temp_db("branchless");
        let store = zet_blocks::Store::open(path.to_str().unwrap()).unwrap();
        store
            .append(&block("s1", "echo alpha", "alpha\n", 0, 3))
            .unwrap();
        drop(store);
        let out = replay_cmd(Some(path.to_str().unwrap()), "s1").unwrap();
        assert!(!out.contains("main"), "{out}");
        assert!(!out.contains('[') || out.contains("[exit"), "{out}");
        std::fs::remove_file(&path).unwrap();
    }
}
