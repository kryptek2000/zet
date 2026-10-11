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
        _ => {
            eprintln!("usage:");
            eprintln!("  zet auth show             list providers and key status");
            eprintln!("  zet auth add <provider>   prompt for key, store in OS keychain");
            eprintln!("  zet auth rm <provider>    delete key from OS keychain");
            eprintln!("  zet blocks [--db PATH] [--limit N] [--session S]");
            eprintln!("                            list recorded command blocks");
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
    eprint!("Paste API key for {provider}: ");
    use std::io::Write as _;
    let _ = std::io::stderr().flush();
    let key = match std::io::stdin().lines().next() {
        Some(Ok(line)) => line,
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
