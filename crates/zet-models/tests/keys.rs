use std::collections::HashMap;
use zet_models::keys::{resolve_key, resolve_provider, EnvKeychain, Keychain};

struct Fake {
    store: HashMap<String, String>,
}

impl Fake {
    fn with(pair: (&str, &str)) -> Self {
        let mut store = HashMap::new();
        store.insert(pair.0.to_string(), pair.1.to_string());
        Fake { store }
    }

    fn empty() -> Self {
        Fake {
            store: HashMap::new(),
        }
    }
}

impl Keychain for Fake {
    fn get(&self, provider: &str) -> Option<String> {
        self.store.get(provider).cloned()
    }
    fn set(&self, _provider: &str, _key: &str) -> Result<(), zet_models::keys::KeyError> {
        panic!("read-only fake")
    }
    fn delete(&self, _provider: &str) -> Result<(), zet_models::keys::KeyError> {
        panic!("read-only fake")
    }
}

/// Array literals don't coerce to `&[&dyn Keychain]`; build the slice via this.
fn chain(fakes: &[Fake]) -> Vec<&dyn Keychain> {
    fakes.iter().map(|f| f as &dyn Keychain).collect()
}

#[test]
fn env_keychain_roundtrip() {
    // Unique provider per test: env is process-global.
    let kc = EnvKeychain;
    kc.set("test_roundtrip_xyz", "k123").unwrap();
    assert_eq!(kc.get("test_roundtrip_xyz").as_deref(), Some("k123"));
    kc.delete("test_roundtrip_xyz").unwrap();
    assert_eq!(kc.get("test_roundtrip_xyz"), None);
}

#[test]
fn missing_key_is_none_not_empty() {
    let kc = EnvKeychain;
    assert_eq!(kc.get("test_absent_xyz"), None);
}

#[test]
fn resolve_key_first_hit_wins() {
    let a = Fake::with(("openrouter", "from-a"));
    let b = Fake::with(("openrouter", "from-b"));
    assert_eq!(
        resolve_key("openrouter", &chain(&[a, b])).as_deref(),
        Some("from-a")
    );
    let b = Fake::with(("openrouter", "from-b"));
    assert_eq!(
        resolve_key("openrouter", &chain(&[b])).as_deref(),
        Some("from-b")
    );
    assert_eq!(resolve_key("openrouter", &chain(&[Fake::empty()])), None);
}

#[test]
fn resolve_openrouter_spec() {
    let fake = Fake::with(("openrouter", "sk-or"));
    let p = resolve_provider("openrouter:anthropic/claude-sonnet", &chain(&[fake])).unwrap();
    match p {
        zet_models::Provider::OpenAICompat {
            base_url,
            api_key,
            model,
        } => {
            assert_eq!(base_url, "https://openrouter.ai/api/v1");
            assert_eq!(api_key, "sk-or");
            assert_eq!(model, "anthropic/claude-sonnet");
        }
        other => panic!("wrong provider: {other:?}"),
    }
}

#[test]
fn resolve_openai_and_compat_and_local_specs() {
    let fake = Fake::with(("openai", "sk-oai"));
    let p = resolve_provider("openai:gpt-4o-mini", &chain(&[fake])).unwrap();
    match p {
        zet_models::Provider::OpenAICompat {
            base_url, model, ..
        } => {
            assert_eq!(base_url, "https://api.openai.com/v1");
            assert_eq!(model, "gpt-4o-mini");
        }
        other => panic!("wrong provider: {other:?}"),
    }

    let fake = Fake::with(("compat", "sk-c"));
    let p = resolve_provider("compat:https://gw.internal/v1:my-model", &chain(&[fake])).unwrap();
    match p {
        zet_models::Provider::OpenAICompat {
            base_url, model, ..
        } => {
            assert_eq!(base_url, "https://gw.internal/v1");
            assert_eq!(model, "my-model");
        }
        other => panic!("wrong provider: {other:?}"),
    }
    // Tagged models on pathed endpoints (same splitter as local:).
    let fake2 = Fake::with(("compat", "sk-c"));
    let p = resolve_provider("compat:http://h:4000/v1:llama:8b", &chain(&[fake2])).unwrap();
    match p {
        zet_models::Provider::OpenAICompat {
            base_url, model, ..
        } => {
            assert_eq!(base_url, "http://h:4000/v1");
            assert_eq!(model, "llama:8b");
        }
        other => panic!("wrong provider: {other:?}"),
    }

    let none: &[&dyn Keychain] = &[];
    let p = resolve_provider("local:http://localhost:11434:qwen3", none).unwrap();
    match p {
        zet_models::Provider::Local { endpoint, model } => {
            assert_eq!(endpoint, "http://localhost:11434");
            assert_eq!(model, "qwen3");
        }
        other => panic!("wrong provider: {other:?}"),
    }
    // Tagged models: default endpoint and explicit endpoint alike.
    let p = resolve_provider("local:qwen3:1.7b", none).unwrap();
    match p {
        zet_models::Provider::Local { endpoint, model } => {
            assert_eq!(endpoint, "http://localhost:11434/v1");
            assert_eq!(model, "qwen3:1.7b");
        }
        other => panic!("wrong provider: {other:?}"),
    }
    let p = resolve_provider("local:http://localhost:11434:qwen3:1.7b", none).unwrap();
    match p {
        zet_models::Provider::Local { endpoint, model } => {
            assert_eq!(endpoint, "http://localhost:11434");
            assert_eq!(model, "qwen3:1.7b");
        }
        other => panic!("wrong provider: {other:?}"),
    }
    let p = resolve_provider("local:qwen3", none).unwrap();
    match p {
        zet_models::Provider::Local { endpoint, model } => {
            assert_eq!(endpoint, "http://localhost:11434/v1");
            assert_eq!(model, "qwen3");
        }
        other => panic!("wrong provider: {other:?}"),
    }
    // Malformed locals stay errors: port without model, IPv6, dangling colon.
    for spec in [
        "local:http://h:11434",
        "local:http://[::1]:11434:x",
        "local:http://h:11434:",
    ] {
        assert!(
            resolve_provider(spec, none).is_err(),
            "{spec} should not parse"
        );
    }
    // Endpoints may carry a path (LiteLLM-style gateways).
    let p = resolve_provider("local:http://h:4000/v1:llama:8b", none).unwrap();
    match p {
        zet_models::Provider::Local { endpoint, model } => {
            assert_eq!(endpoint, "http://h:4000/v1");
            assert_eq!(model, "llama:8b");
        }
        other => panic!("wrong provider: {other:?}"),
    }
}

#[test]
fn resolve_missing_key_names_the_provider() {
    let err = resolve_provider("openrouter:x/y", &chain(&[Fake::empty()])).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("openrouter"), "{msg}");
    assert!(msg.contains("zet auth add"), "{msg}");
}

#[test]
fn resolve_garbage_spec_is_an_error() {
    for spec in ["", "noscheme", "compat:onlyonepart", "mystery:model"] {
        assert!(
            resolve_provider(spec, &chain(&[Fake::empty()])).is_err(),
            "{spec}"
        );
    }
}

// Live: needs an unlocked OS keychain (desktop session). CI skips;
// run locally with `cargo test -p zet-models -- --ignored`.
#[test]
#[ignore]
fn system_keychain_live_roundtrip() {
    use zet_models::keys::{Keychain, SystemKeychain};
    let kc = SystemKeychain;
    let account = format!("test_live_{}", std::process::id());
    kc.set(&account, "s3cr3t").unwrap();
    assert_eq!(kc.get(&account).as_deref(), Some("s3cr3t"));
    kc.delete(&account).unwrap();
    assert_eq!(kc.get(&account), None);
}

fn opencode_scratch(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "zet-opencode-{}-{}-{:?}",
        name,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p.join("auth.json")
}

#[test]
fn opencode_key_roundtrip_and_shapes() {
    use zet_models::keys::read_opencode_key_at;
    // Missing, malformed, wrong shape: all None (fall through).
    let f = opencode_scratch("missing");
    assert_eq!(read_opencode_key_at(&f), None);
    let f = opencode_scratch("bad");
    std::fs::write(&f, "not json{{").unwrap();
    assert_eq!(read_opencode_key_at(&f), None);
    let f = opencode_scratch("shape");
    std::fs::write(&f, r#"{"other": {"key": "x"}}"#).unwrap();
    assert_eq!(read_opencode_key_at(&f), None);
    // Blank key = unset.
    let f = opencode_scratch("blank");
    std::fs::write(&f, r#"{"opencode-go": {"type": "oauth", "key": "  "}}"#).unwrap();
    assert_eq!(read_opencode_key_at(&f), None);
    // Real shape reads.
    let f = opencode_scratch("good");
    std::fs::write(
        &f,
        r#"{"opencode-go": {"type": "oauth", "key": "oc_sk_test"}}"#,
    )
    .unwrap();
    assert_eq!(read_opencode_key_at(&f).as_deref(), Some("oc_sk_test"));
}

#[test]
fn opencode_resolve_prefers_file_then_keychain() {
    use zet_models::keys::{resolve_opencode, ResolveError};
    use zet_models::Provider;
    let file = opencode_scratch("resolve");
    std::fs::write(&file, r#"{"opencode-go": {"key": "oc_sk_file"}}"#).unwrap();
    // File wins over keychain.
    let p = resolve_opencode(
        "muse-spark",
        &chain(&[Fake::with(("opencode", "chain-key"))]),
        &file,
    )
    .unwrap();
    match p {
        Provider::Go { api_key, model } => {
            assert_eq!(api_key, "oc_sk_file");
            assert_eq!(model, "muse-spark");
        }
        _ => panic!("wrong provider"),
    }
    // No file: keychain fallback.
    let missing = opencode_scratch("nope-missing");
    let p = resolve_opencode(
        "muse-spark",
        &chain(&[Fake::with(("opencode", "chain-key"))]),
        &missing,
    )
    .unwrap();
    match p {
        Provider::Go { api_key, .. } => assert_eq!(api_key, "chain-key"),
        _ => panic!("wrong provider"),
    }
    // Neither: login advice, and it names the CLI command.
    let err = resolve_opencode("muse-spark", &chain(&[Fake::empty()]), &missing).unwrap_err();
    assert!(matches!(err, ResolveError::OpencodeLogin), "{err:?}");
    assert!(err.to_string().contains("opencode auth login"), "{err}");
    // Empty model: bad spec.
    assert!(resolve_opencode("", &chain(&[Fake::empty()]), &missing).is_err());
}

#[test]
fn opencode_auth_path_default_shape() {
    use zet_models::keys::opencode_auth_path;
    assert!(opencode_auth_path().ends_with("opencode/auth.json"));
}
