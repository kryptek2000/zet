use std::path::PathBuf;
use zet_models::keys::{
    clear_inline_spec_at, inline_spec_path, read_inline_spec_at, resolve_inline_spec,
    write_inline_spec_at,
};

fn scratch() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "zet-inline-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p.join("inline_spec")
}

#[test]
fn roundtrip_write_read_clear() {
    let f = scratch();
    assert_eq!(read_inline_spec_at(&f), None);
    write_inline_spec_at(&f, "openrouter:openai/gpt-4o-mini\n").unwrap();
    assert_eq!(
        read_inline_spec_at(&f),
        Some("openrouter:openai/gpt-4o-mini".to_string())
    );
    // Blank file reads as unset, never as empty spec.
    write_inline_spec_at(&f, "  \n").unwrap();
    assert_eq!(read_inline_spec_at(&f), None);
    clear_inline_spec_at(&f).unwrap();
    assert_eq!(read_inline_spec_at(&f), None);
    // Clearing a missing file is fine.
    clear_inline_spec_at(&f).unwrap();
}

#[test]
fn precedence_env_then_file_then_startup() {
    let f = scratch();
    // Nothing anywhere: None (caller falls through to fresh autodetect).
    assert_eq!(resolve_inline_spec(None, &f, None), None);
    // Startup fallback only.
    assert_eq!(
        resolve_inline_spec(None, &f, Some("local:llama3.2:1b".to_string())),
        Some("local:llama3.2:1b".to_string())
    );
    // File beats startup.
    write_inline_spec_at(&f, "openrouter:openai/gpt-4o-mini").unwrap();
    assert_eq!(
        resolve_inline_spec(None, &f, Some("local:llama3.2:1b".to_string())),
        Some("openrouter:openai/gpt-4o-mini".to_string())
    );
    // Explicit env beats everything (launch config wins).
    assert_eq!(
        resolve_inline_spec(
            Some("compat:custom".to_string()),
            &f,
            Some("local:llama3.2:1b".to_string())
        ),
        Some("compat:custom".to_string())
    );
}

#[test]
fn default_path_lives_beside_journal() {
    // Same ~/.zet home as the journal: one state dir, no new convention.
    assert!(inline_spec_path().ends_with(".zet/inline_spec"));
}
