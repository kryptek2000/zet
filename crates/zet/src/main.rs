use std::io::Read;
use zet_models::keys::{EnvKeychain, Keychain, SystemKeychain};

const PROVIDERS: &[&str] = &["openai", "openrouter", "compat"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["auth", "show"] => auth_show(),
        ["auth", "add", provider] => auth_add(provider),
        ["auth", "rm", provider] => auth_rm(provider),
        _ => {
            eprintln!("usage:");
            eprintln!("  zet auth show             list providers and key status");
            eprintln!("  zet auth add <provider>   read key from stdin, store in OS keychain");
            eprintln!("  zet auth rm <provider>    delete key from OS keychain");
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
    let mut key = String::new();
    if std::io::stdin().read_to_string(&mut key).is_err() {
        eprintln!("failed to read key from stdin");
        std::process::exit(1);
    }
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
