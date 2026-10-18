// The gate and the store must agree on what a credential is.
//
// zet-blocks redacts key-like text at the single write path; zet-policy
// decides whether the command carrying it may run. If the store redacts a
// token the gate let through, the transcript reads "[REDACTED]" while the
// machine did the thing — the red team caught exactly that disagreement
// (quoted keys, and a Bearer threshold of 16 here against 8 there).
//
// This test walks every tail length around both thresholds: whatever the
// scrubber hides, the gate must refuse.

use zet_policy::{classify, Tier};

fn prefixes() -> Vec<(&'static str, usize)> {
    // (prefix, minimum tail the scrubber needs) — mirrors
    // zet_blocks::scrub_secrets' SECRET_PREFIXES table.
    vec![
        ("sk-or-", 16),
        ("sk-ant-", 16),
        ("sk-proj-", 16),
        ("github_pat_", 16),
        ("xoxb-", 16),
        ("xoxp-", 16),
        ("xoxa-", 16),
        ("xoxo-", 16),
        ("ghp_", 16),
        ("gho_", 16),
        ("AKIA", 16),
        ("sk-", 16),
        ("Bearer ", 8),
    ]
}

#[test]
fn everything_the_scrubber_hides_is_refused_by_the_gate() {
    for (prefix, min_tail) in prefixes() {
        // The table above claims the scrubber's own rule for that prefix:
        // at its documented tail it redacts. (There is deliberately no
        // matching "one below does not" assertion — prefixes overlap on
        // purpose: `sk-or-<15>` is caught by `sk-`, not by `sk-or-`.)
        let at = format!("echo {prefix}{}", "a".repeat(min_tail));
        assert_ne!(
            zet_blocks::scrub_secrets(&at),
            at,
            "{prefix:?} does not redact at its documented tail of {min_tail}"
        );
        for tail in 0..24 {
            for quoted in ["", "'", "\""] {
                let token = format!("{quoted}{prefix}{}{quoted}", "a".repeat(tail));
                let cmd = format!("echo {token}");
                let scrubbed = zet_blocks::scrub_secrets(&cmd);
                if scrubbed == cmd {
                    continue; // store keeps it: the gate need not care
                }
                assert!(
                    classify(&cmd) >= Tier::T3,
                    "the store redacts {cmd:?} (as {scrubbed:?}) but the gate allows it"
                );
            }
        }
    }
}

#[test]
fn the_var_assignment_form_is_covered_too() {
    for tail in 0..24 {
        let token = format!("sk-{}", "a".repeat(tail));
        let cmd = format!("export K='{token}'");
        if zet_blocks::scrub_secrets(&cmd) != cmd {
            assert!(
                classify(&cmd) >= Tier::T3,
                "store redacts {cmd:?} but the gate allows it"
            );
        }
    }
}

/// The other direction, pinned so the fix cannot degrade into a
/// blanket "anything long is a secret": ordinary text stays clean.
#[test]
fn ordinary_text_is_not_a_credential() {
    for cmd in [
        "echo hello",
        "echo sk-folders",
        "git commit -m 'secret fix'",
        "ls -la",
    ] {
        assert_eq!(
            zet_blocks::scrub_secrets(cmd),
            cmd,
            "scrubber over-reached on {cmd:?}"
        );
    }
}
