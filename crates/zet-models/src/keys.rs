// API-key storage and provider-spec resolution.
// Keys live in the OS keychain (SystemKeychain) with an env-var override
// (EnvKeychain) for headless/CI. Resolution order is caller-chosen via
// `resolve_key`: env first, then system. Keys never touch disk in this crate.

use crate::Provider;

#[derive(Debug)]
pub enum KeyError {
    Backend(String),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::Backend(e) => write!(f, "keychain backend: {e}"),
        }
    }
}

impl std::error::Error for KeyError {}

pub trait Keychain {
    /// None when no key is stored. Never returns an empty string.
    fn get(&self, provider: &str) -> Option<String>;
    fn set(&self, provider: &str, key: &str) -> Result<(), KeyError>;
    fn delete(&self, provider: &str) -> Result<(), KeyError>;
}

/// `ZET_<PROVIDER>_API_KEY` (uppercased, dashes to underscores).
/// Process env is global: tests must use unique provider names.
pub struct EnvKeychain;

impl EnvKeychain {
    pub fn var_name(provider: &str) -> String {
        format!("ZET_{}_API_KEY", provider.to_uppercase().replace('-', "_"))
    }
}

impl Keychain for EnvKeychain {
    fn get(&self, provider: &str) -> Option<String> {
        std::env::var(Self::var_name(provider))
            .ok()
            .filter(|k| !k.is_empty())
    }

    fn set(&self, provider: &str, key: &str) -> Result<(), KeyError> {
        // SAFETY: process-env write; tests use unique providers.
        unsafe { std::env::set_var(Self::var_name(provider), key) };
        Ok(())
    }

    fn delete(&self, provider: &str) -> Result<(), KeyError> {
        // SAFETY: process-env write; tests use unique providers.
        unsafe { std::env::remove_var(Self::var_name(provider)) };
        Ok(())
    }
}

/// First non-empty hit wins. Callers pass env before system.
pub fn resolve_key(provider: &str, chains: &[&dyn Keychain]) -> Option<String> {
    chains
        .iter()
        .find_map(|c| c.get(provider).filter(|k| !k.is_empty()))
}

/// OS keychain via the `keyring` crate (service `zet`, account = provider).
/// Headless without a secret service: every op degrades to None/Err, never
/// panics — callers fall back to [`EnvKeychain`].
pub struct SystemKeychain;

impl SystemKeychain {
    const SERVICE: &str = "zet";

    fn entry(provider: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(Self::SERVICE, provider).ok()
    }
}

impl Keychain for SystemKeychain {
    fn get(&self, provider: &str) -> Option<String> {
        Self::entry(provider)?.get_password().ok()
    }

    fn set(&self, provider: &str, key: &str) -> Result<(), KeyError> {
        Self::entry(provider)
            .ok_or_else(|| KeyError::Backend("no secret service".to_string()))?
            .set_password(key)
            .map_err(|e| KeyError::Backend(e.to_string()))
    }

    fn delete(&self, provider: &str) -> Result<(), KeyError> {
        let Some(entry) = Self::entry(provider) else {
            return Ok(());
        };
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            // Idempotent delete: missing is already gone.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeyError::Backend(e.to_string())),
        }
    }
}

#[derive(Debug)]
pub enum ResolveError {
    BadSpec(String),
    MissingKey { provider: String },
    OpencodeLogin,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::BadSpec(s) => write!(
                f,
                "bad model spec {s:?}: want openai:MODEL, openrouter:MODEL, compat:URL:MODEL, opencode:MODEL, or local[:ENDPOINT]:MODEL"
            ),
            ResolveError::MissingKey { provider } => write!(
                f,
                "no API key for '{provider}': run `zet auth add {provider}` or set {}",
                EnvKeychain::var_name(provider)
            ),
            ResolveError::OpencodeLogin => write!(
                f,
                "no OpenCode Go credential: run `opencode auth login opencode`, or `zet auth add opencode`"
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

fn need_key(provider: &str, chains: &[&dyn Keychain]) -> Result<String, ResolveError> {
    resolve_key(provider, chains).ok_or(ResolveError::MissingKey {
        provider: provider.to_string(),
    })
}

/// Models preferred for inline use, fastest first. Skips embedding and
/// unlisted models (a wrong pick here means garbage suggestions).
const LOCAL_PREFERRED: &[&str] = &["llama3.2:1b", "qwen3:1.7b", "llama3.2:3b", "qwen2.5:1.5b"];

/// Zero-config local default: `local:<model>` when a preferred model is
/// serving on localhost:11434 (ollama convention), else None. Fast fail:
/// 100ms TCP probe, 2s tags fetch. No key, localhost only.
pub fn autodetect_local() -> Option<String> {
    autodetect_local_at("127.0.0.1", 11434)
}

/// Home for the persisted NL/agent model choice (`zet inline <spec>`):
/// same ~/.zet state dir as the journal. A child shell can never change
/// its parent window's env, so the window re-reads this file on every
/// NL/agent request — no restart needed after `zet inline`.
pub fn inline_spec_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::Path::new(&home).join(".zet").join("inline_spec")
}

/// First line, trimmed. Blank/missing reads as unset, never empty spec.
pub fn read_inline_spec_at(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let first = text.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        None
    } else {
        Some(first.to_string())
    }
}

pub fn write_inline_spec_at(path: &std::path::Path, spec: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{}\n", spec.trim()))
}

/// Removing a missing file is fine (already off).
pub fn clear_inline_spec_at(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Precedence: explicit env (launch config) > persisted file > the
/// window's startup value. None everywhere means the caller should try
/// a fresh autodetect (server may have started since launch).
pub fn resolve_inline_spec(
    env: Option<String>,
    path: &std::path::Path,
    startup: Option<String>,
) -> Option<String> {
    env.or_else(|| read_inline_spec_at(path)).or(startup)
}

pub fn autodetect_local_at(host: &str, port: u16) -> Option<String> {
    // Loopback only: a future caller can't aim this elsewhere.
    let ip: std::net::IpAddr = host.parse().ok()?;
    if !ip.is_loopback() {
        return None;
    }
    let names = local_tags_at(host, port)?;
    LOCAL_PREFERRED
        .iter()
        .find(|p| names.iter().any(|n| n == *p))
        .map(|s| format!("local:{s}"))
}

/// Every model the local server reports (ollama `/api/tags`), for
/// `zet models`. None when unreachable — same fast-fail bounds as
/// autodetect (100ms probe, 2s fetch), loopback only.
pub fn local_tags_at(host: &str, port: u16) -> Option<Vec<String>> {
    let ip: std::net::IpAddr = host.parse().ok()?;
    if !ip.is_loopback() {
        return None;
    }
    let addr = format!("{host}:{port}");
    let sock: std::net::SocketAddr = addr.parse().ok()?;
    std::net::TcpStream::connect_timeout(&sock, std::time::Duration::from_millis(100)).ok()?;
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(2)))
            .build(),
    );
    let mut body = agent.get(format!("http://{addr}/api/tags")).call().ok()?;
    let text = body.body_mut().read_to_string().ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let names: Vec<String> = json
        .get("models")?
        .as_array()?
        .iter()
        .filter_map(|m| m.get("name")?.as_str().map(str::to_string))
        .collect();
    Some(names)
}

/// Left side of a `local:` split: scheme://host[:port][/path], no
/// brackets, numeric port, colon-free path.
fn valid_base(url: &str) -> bool {
    let Some(tail) = url.split_once("://").map(|(_, t)| t) else {
        return false;
    };
    if tail.is_empty() {
        return false;
    }
    let (authority, path) = match tail.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (tail, None),
    };
    let mut parts = authority.split(':');
    let host = parts.next().unwrap_or("");
    if host.is_empty() || host.contains(['[', ']']) {
        return false;
    }
    match parts.next() {
        None => {}
        Some(port) if port.is_empty() || !port.bytes().all(|c| c.is_ascii_digit()) => {
            return false;
        }
        Some(_) => {}
    }
    if parts.next().is_some() {
        return false;
    }
    match path {
        None => true,
        Some(p) => !p.contains(':'),
    }
}

/// Right side of an endpoint split: present, not a bare port number,
/// no dangling separator, no path slashes (those belong to the endpoint).
fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && !model.bytes().all(|c| c.is_ascii_digit())
        && !model.ends_with(':')
        && !model.contains('/')
}

/// Shared endpoint splitter: every ':' right-to-left on the ORIGINAL
/// rest (a walk that truncates would forget right context). Takes the
/// first valid split whose model isn't port-headed.
fn split_endpoint_model(rest: &str) -> Option<(&str, &str)> {
    let splits: Vec<usize> = rest.match_indices(':').map(|(i, _)| i).collect();
    for i in splits.into_iter().rev() {
        let (left, model) = (&rest[..i], &rest[i + 1..]);
        if valid_base(left) && valid_model(model) && !port_headed(model) {
            return Some((left, model));
        }
    }
    None
}

/// A model starting `digits:` was cut inside the endpoint
/// (`http://h:11434` + `:qwen3`, not endpoint `http://h`).
fn port_headed(model: &str) -> bool {
    match model.split_once(':') {
        Some((p, _)) => !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

/// Build a [`Provider`] from a config spec, fetching the key from `chains`:
/// `openai:MODEL`, `openrouter:VENDOR/MODEL`, `compat:BASE_URL:MODEL`
/// (split on the last `:` so URLs survive), `opencode:MODEL` (Go
/// subscription via the opencode CLI login, else `opencode` keychain),
/// `local:MODEL` or `local:ENDPOINT:MODEL` (no key needed).
pub fn resolve_provider(spec: &str, chains: &[&dyn Keychain]) -> Result<Provider, ResolveError> {
    let bad = || ResolveError::BadSpec(spec.to_string());
    let (kind, rest) = spec.split_once(':').ok_or_else(bad)?;
    if rest.is_empty() {
        return Err(bad());
    }
    match kind {
        "openai" => Ok(Provider::OpenAICompat {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: need_key("openai", chains)?,
            model: rest.to_string(),
        }),
        "openrouter" => Ok(Provider::openrouter(
            need_key("openrouter", chains)?,
            rest.to_string(),
        )),
        "compat" => {
            let (url, model) = split_endpoint_model(rest).ok_or_else(bad)?;
            if url.is_empty() || model.is_empty() {
                return Err(bad());
            }
            Ok(Provider::OpenAICompat {
                base_url: url.trim_end_matches('/').to_string(),
                api_key: need_key("compat", chains)?,
                model: model.to_string(),
            })
        }
        "opencode" => resolve_opencode(rest, chains, &opencode_auth_path()),
        "local" => {
            // Bare model on the default endpoint (tags may contain
            // colons), or ENDPOINT:MODEL with an explicit base URL:
            // the rightmost split whose left side is a valid base URL
            // (scheme://host[:port][/path]) and whose model is non-empty,
            // non-numeric, and colon-clean at the edges.
            match rest.split_once("://") {
                None => Ok(Provider::Local {
                    endpoint: "http://localhost:11434/v1".to_string(),
                    model: rest.to_string(),
                }),
                Some(_) => match split_endpoint_model(rest) {
                    Some((endpoint, model)) => Ok(Provider::Local {
                        endpoint: endpoint.to_string(),
                        model: model.to_string(),
                    }),
                    None => Err(bad()),
                },
            }
        }
        _ => Err(bad()),
    }
}

/// Where the opencode CLI keeps its login (`~/.local/share/opencode`,
/// or XDG_DATA_HOME when set).
pub fn opencode_auth_path() -> std::path::PathBuf {
    let data = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.local/share")
    });
    std::path::Path::new(&data)
        .join("opencode")
        .join("auth.json")
}

/// Go subscription key from an opencode auth file (`{"opencode-go":
/// {"key": "oc_sk_…", …}}`). None on missing/unreadable/misshapen —
/// callers fall through to the `opencode` keychain, then login advice.
pub fn read_opencode_key_at(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let key = json.pointer("/opencode-go/key")?.as_str()?;
    if key.trim().is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

/// `opencode:MODEL`: OpenCode Go gateway with the CLI login's key,
/// else a pasted `opencode` keychain entry. Path is a parameter so
/// tests stay hermetic (no real HOME reads).
pub fn resolve_opencode(
    model: &str,
    chains: &[&dyn Keychain],
    auth_path: &std::path::Path,
) -> Result<Provider, ResolveError> {
    if model.is_empty() {
        return Err(ResolveError::BadSpec("opencode:".to_string()));
    }
    let key = read_opencode_key_at(auth_path).or_else(|| resolve_key("opencode", chains));
    let Some(key) = key else {
        return Err(ResolveError::OpencodeLogin);
    };
    Ok(Provider::OpenAICompat {
        base_url: "https://opencode.ai/zen/go/v1".to_string(),
        api_key: key,
        model: model.to_string(),
    })
}
