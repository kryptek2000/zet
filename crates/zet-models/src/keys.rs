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
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::BadSpec(s) => write!(
                f,
                "bad model spec {s:?}: want openai:MODEL, openrouter:MODEL, compat:URL:MODEL, or local[:ENDPOINT]:MODEL"
            ),
            ResolveError::MissingKey { provider } => write!(
                f,
                "no API key for '{provider}': run `zet auth add {provider}` or set {}",
                EnvKeychain::var_name(provider)
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

/// Build a [`Provider`] from a config spec, fetching the key from `chains`:
/// `openai:MODEL`, `openrouter:VENDOR/MODEL`, `compat:BASE_URL:MODEL`
/// (split on the last `:` so URLs survive), `local:MODEL` or
/// `local:ENDPOINT:MODEL` (no key needed).
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
            let (url, model) = rest.rsplit_once(':').ok_or_else(bad)?;
            if url.is_empty() || model.is_empty() {
                return Err(bad());
            }
            Ok(Provider::OpenAICompat {
                base_url: url.trim_end_matches('/').to_string(),
                api_key: need_key("compat", chains)?,
                model: model.to_string(),
            })
        }
        "local" => {
            // Bare model on the default endpoint (tags may contain
            // colons), or ENDPOINT:MODEL where the endpoint carries a
            // scheme: the port, when numeric, separates endpoint from
            // model; otherwise the whole tail after host is the model.
            match rest.split_once("://") {
                None => Ok(Provider::Local {
                    endpoint: "http://localhost:11434".to_string(),
                    model: rest.to_string(),
                }),
                Some((scheme, tail)) => {
                    let (host, after_host) = tail.split_once(':').ok_or_else(bad)?;
                    if host.is_empty() || host.contains(['[', ']']) {
                        return Err(bad());
                    }
                    let (port, model) = match after_host.split_once(':') {
                        Some((p, m))
                            if !p.is_empty()
                                && p.bytes().all(|c| c.is_ascii_digit())
                                && !m.is_empty() =>
                        {
                            (format!(":{p}"), m)
                        }
                        _ if after_host.bytes().all(|c| c.is_ascii_digit())
                            && !after_host.is_empty() =>
                        {
                            return Err(bad());
                        }
                        // Dangling separator: port present, model missing.
                        _ if after_host.ends_with(':') => {
                            return Err(bad());
                        }
                        _ => (String::new(), after_host),
                    };
                    if model.is_empty() {
                        return Err(bad());
                    }
                    Ok(Provider::Local {
                        endpoint: format!("{scheme}://{host}{port}"),
                        model: model.to_string(),
                    })
                }
            }
        }
        _ => Err(bad()),
    }
}
