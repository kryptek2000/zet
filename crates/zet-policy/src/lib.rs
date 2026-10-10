// Tiered command policy: classify shell commands T0-T3, map tiers to
// approval decisions. Pure function of (command, in_repo) — no IO, so the
// red-team suite can pin every attack to a tier.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Read-only: auto-approved.
    T0,
    /// Low-write: auto in repo, ask outside.
    T1,
    /// Risky: always ask, show exact command + diff.
    T2,
    /// Critical: ask + type-to-confirm. Never auto-passes.
    T3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Auto,
    Ask,
    ConfirmType,
}

pub fn decide(tier: Tier, in_repo: bool) -> Approval {
    match tier {
        Tier::T0 => Approval::Auto,
        Tier::T1 if in_repo => Approval::Auto,
        Tier::T1 => Approval::Ask,
        Tier::T2 => Approval::Ask,
        Tier::T3 => Approval::ConfirmType,
    }
}

fn first_token(cmd: &str) -> &str {
    cmd.split_whitespace().next().unwrap_or("")
}

/// Path-like tokens that smell like secrets. Requires a `/`, `.`, or `~`
/// so commit messages like "secret fix" don't false-positive.
fn is_secret_token(tok: &str) -> bool {
    let lower = tok.to_lowercase();
    let pathy = tok.contains('/') || tok.contains('.') || tok.contains('~');
    pathy
        && (lower.contains(".ssh")
            || lower.contains(".aws")
            || lower.contains("id_rsa")
            || lower.contains("id_ed25519")
            || lower.contains("credentials")
            || lower.contains(".gnupg")
            || lower.contains("secret")
            || lower.contains("token")
            || lower.contains(".pem")
            || lower.contains(".key"))
}

fn kubectl_tier(cmd: &str, lower: &str) -> Tier {
    if lower.contains("prod") {
        return Tier::T3;
    }
    let sub = cmd.split_whitespace().nth(1).unwrap_or("");
    match sub {
        "get" | "describe" | "logs" | "top" => Tier::T0,
        "apply" | "delete" | "scale" | "exec" | "cp" | "port-forward" | "drain" => Tier::T2,
        _ => Tier::T1,
    }
}

fn git_tier(cmd: &str) -> Tier {
    let sub = cmd.split_whitespace().nth(1).unwrap_or("");
    match sub {
        "status" | "diff" | "log" | "show" | "branch" | "rev-parse" => Tier::T0,
        "push" => Tier::T2,
        _ => Tier::T1,
    }
}

pub fn classify(cmd: &str) -> Tier {
    let lower = cmd.to_lowercase();

    // --- T3: critical. Checked first; nothing below may downgrade these. ---
    if lower.contains("sudo") {
        return Tier::T3;
    }
    if lower.contains(":(){") || lower.contains("mkfs") || lower.starts_with("dd ") {
        return Tier::T3;
    }
    // Mass delete: rm -rf aimed at root, home, or everything.
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    if toks.first().is_some_and(|t| t.eq_ignore_ascii_case("rm"))
        && toks
            .iter()
            .any(|t| *t == "-rf" || *t == "-fr" || *t == "-r")
    {
        let targets: Vec<&&str> = toks
            .iter()
            .skip(1)
            .filter(|t| !t.starts_with('-'))
            .collect();
        let mass = targets.iter().any(|t| {
            let t = t.trim_matches('\'').trim_matches('"');
            matches!(
                t,
                "/" | "/*" | "~" | "~/*" | "." | "./" | "*" | "$HOME" | "$HOME/*"
            )
        });
        if mass || targets.is_empty() {
            return Tier::T3;
        }
        return Tier::T2;
    }
    if cmd.split_whitespace().any(is_secret_token) {
        return Tier::T3;
    }
    if first_token(&lower) == "env" || first_token(&lower) == "printenv" {
        return Tier::T3;
    }
    if first_token(&lower) == "kubectl" {
        return kubectl_tier(cmd, &lower);
    }

    // --- T2: risky. ---
    if lower.contains("| sh")
        || lower.contains("| bash")
        || lower.contains("| sudo")
        || lower.contains("|sudo")
    {
        return Tier::T2;
    }
    let first = first_token(&lower);
    if first == "docker" {
        let sub = cmd.split_whitespace().nth(1).unwrap_or("").to_lowercase();
        return match sub.as_str() {
            "ps" | "images" | "ls" => Tier::T0,
            _ => Tier::T2,
        };
    }
    if first == "git" {
        let t = git_tier(cmd);
        if t == Tier::T2 {
            return Tier::T2;
        }
    }

    // --- T1: writes and unknowns. ---
    // Redirection writes a file (ignoring 2> stderr noise).
    let no_stderr = lower.replace("2>", "\u{0}");
    if no_stderr.contains('>') {
        return Tier::T1;
    }
    if first == "git" {
        return git_tier(cmd);
    }
    if matches!(
        first,
        "npm" | "cargo" | "pip" | "mkdir" | "touch" | "cp" | "mv" | "sed" | "tee" | "curl" | "wget"
    ) {
        return Tier::T1;
    }
    if first == "rm" {
        return Tier::T1;
    }

    // --- T0: known reads. ---
    if matches!(
        first,
        "ls" | "cat"
            | "head"
            | "tail"
            | "less"
            | "more"
            | "echo"
            | "pwd"
            | "ps"
            | "which"
            | "file"
            | "wc"
            | "grep"
            | "true"
            | "false"
            | "whoami"
            | "uname"
            | "date"
    ) {
        return Tier::T0;
    }

    // Cautious default: ask.
    Tier::T1
}
