// Tiered command policy: classify shell commands T0-T3, map tiers to
// approval decisions. Pure function of (command, in_repo) — no IO, so the
// red-team suite can pin every attack to a tier.

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// Strip leading `VAR=value` assignments so env prefixes can't shift the
/// real command out of first-token checks. Returns a subslice of `cmd`.
fn strip_env_assignments(cmd: &str) -> &str {
    let mut s = cmd.trim_start();
    loop {
        let tok = s.split_whitespace().next().unwrap_or("");
        if tok.is_empty() {
            break;
        }
        let valid = match tok.split_once('=') {
            Some((name, _)) => {
                !name.is_empty()
                    && !name.contains('/')
                    && !name.starts_with('-')
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !name.starts_with(|c: char| c.is_ascii_digit())
            }
            None => false,
        };
        if !valid {
            break;
        }
        s = s[tok.len()..].trim_start();
    }
    s
}

/// First token, basenamed: `/usr/bin/su` and `./su` both yield `su`.
fn norm_first(core: &str) -> &str {
    let tok = core.split_whitespace().next().unwrap_or("");
    tok.rsplit('/').next().unwrap_or(tok)
}

/// Split on top-level `;`, `&&`, `||` — outside quotes, backslash-aware.
/// Single `&` and `|` are left alone (background/pipe, handled elsewhere).
fn split_chain(cmd: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    let mut escape = false;
    let mut iter = cmd.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if escape {
            escape = false;
            continue;
        }
        if let Some(q) = quote {
            if c == '\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '\\' => escape = true,
            ';' => {
                parts.push(&cmd[start..i]);
                start = i + 1;
            }
            '&' | '|' if iter.peek().is_some_and(|(_, n)| *n == c) => {
                iter.next();
                parts.push(&cmd[start..i]);
                start = i + c.len_utf8() * 2;
            }
            _ => {}
        }
    }
    parts.push(&cmd[start..]);
    parts
}

/// Strip env assignments and transparent wrappers, returning the remaining
/// command. Case-insensitive on wrapper names (`COMMAND su` is still a wrap).
fn strip_wrappers(mut s: &str) -> &str {
    const WRAPPERS: &[&str] = &[
        "command", "builtin", "exec", "time", "nohup", "nice", "setsid", "timeout",
    ];
    loop {
        s = strip_env_assignments(s);
        let tok = s.split_whitespace().next().unwrap_or("");
        if !WRAPPERS
            .iter()
            .any(|w| norm_first(tok).eq_ignore_ascii_case(w))
        {
            return s;
        }
        s = skip_flag_args(s[tok.len()..].trim_start());
    }
}

/// Transparent wrappers resolved past (`timeout 10`, `nice -n 5`).
/// Returns the first REAL token.
fn resolve_first(core: &str) -> &str {
    norm_first(strip_wrappers(core))
}

/// Pipe-segment command with dispatcher layers resolved to fixpoint
/// (`| xargs busybox su` sees `su`, `| xargs grep` sees `grep`).
/// Terminates: each layer consumes at least one token.
fn seg_first(seg: &str) -> &str {
    let mut s = seg;
    loop {
        let f = resolve_first(s);
        if !matches!(f, "xargs" | "busybox") {
            return f;
        }
        let stripped = strip_wrappers(s);
        let tok = stripped.split_whitespace().next().unwrap_or("");
        let rest = skip_flag_args(stripped[tok.len()..].trim_start());
        if rest.trim().is_empty() {
            return f;
        }
        s = rest;
    }
}

/// Flags that consume the following token as their value (`-I REPL`,
/// `--signal KILL`). Case-SENSITIVE, matched against original-case tokens:
/// bare `-i` is boolean (only attached `-iREPL` consumes), while `-I`
/// always takes a separate value. (`-p` prompt must not match `-P`.)
const VALUE_FLAGS: &[&str] = &[
    "-s",
    "--signal",
    "-n",
    "--adjustment",
    "--max-args",
    "-L",
    "-l",
    "--max-lines",
    "-P",
    "--max-procs",
    "--max-chars",
    "-I",
    "--replace",
    "-d",
    "--delimiter",
    "-a",
    "--arg-file",
    "-E",
    "--eof",
];

/// Skip flag (`-x`), value-flag pairs (`-I REPL`), and bare-number (`10`)
/// argument tokens.
fn skip_flag_args(s: &str) -> &str {
    skip_flag_args_with(s, &[])
}

/// Flag/arg skipper with tool-scoped extra value flags. Scoped on
/// purpose: bare `-t` is valueless for xargs (trace) but takes a pid
/// for nsenter — sharing one list would mis-skip one of them.
fn skip_flag_args_with<'a>(mut s: &'a str, extra_values: &[&str]) -> &'a str {
    let mut value_next = false;
    loop {
        let a = s.split_whitespace().next().unwrap_or("");
        if a.is_empty() {
            break;
        }
        if value_next {
            // Previous token was a value-taking flag: this is its value.
            value_next = false;
        } else if VALUE_FLAGS.contains(&a) || extra_values.contains(&a) {
            value_next = true;
        } else if !(a.starts_with('-') || a.chars().all(|c| c.is_ascii_digit())) {
            break;
        }
        s = s[a.len()..].trim_start();
    }
    s
}

fn is_mass_target(t: &str) -> bool {
    let t = t.trim_matches('\'').trim_matches('"');
    matches!(
        t,
        "/" | "/*" | "~" | "~/*" | "~/" | "." | "./" | "*" | "$HOME" | "$HOME/*" | "$HOME/"
    )
}

pub fn classify(cmd: &str) -> Tier {
    // Chains take the worst tier. Recursion terminates: segments hold no
    // top-level separators by construction.
    let segments = split_chain(cmd);
    if segments.len() > 1 {
        return segments
            .iter()
            .filter(|s| !s.trim().is_empty())
            .map(|s| classify(s))
            .max()
            .unwrap_or(Tier::T1);
    }
    // Analyze the command past env prefixes and transparent wrappers:
    // `VAR=1 timeout 10 su ...` must see `su`. Name matching is on
    // ORIGINAL case: the shell itself is case-sensitive, so `SU` or `RM`
    // are not the tools (`lower` survives only for safe over-match
    // substrings like sudo/prod, where extra hits merely over-ask).
    let core = strip_env_assignments(cmd);
    let lower = core.to_lowercase();
    // Basenamed, wrapper-resolved first token, original case.
    let first = resolve_first(core);
    // Dispatchers: the real command follows (xargs may interleave flags).
    // Recursion terminates: the remainder is strictly shorter.
    if matches!(first, "busybox" | "xargs") {
        let resolved = strip_wrappers(core);
        let tok = resolved.split_whitespace().next().unwrap_or("");
        let rest = skip_flag_args(resolved[tok.len()..].trim_start());
        if rest.trim().is_empty() {
            return Tier::T0;
        }
        return classify(rest);
    }
    // Altered roots/namespaces: resolve the inner command past flags and
    // the root/pid target, floor at T2 (crossing the boundary always asks,
    // but `su` inside stays critical). Fails closed: unparseable shapes
    // and bare entry (interactive jail shell) stay T2.
    if matches!(first, "chroot" | "nsenter") {
        let extra = if first == "chroot" {
            &["--userspec", "--groups"][..]
        } else {
            &[
                "-t", "--target", "-r", "--root", "-w", "--wd", "-S", "--setuid", "-G", "--setgid",
            ][..]
        };
        let stripped = strip_wrappers(core);
        let tok = stripped.split_whitespace().next().unwrap_or("");
        let mut rest = skip_flag_args_with(stripped[tok.len()..].trim_start(), extra);
        if first == "chroot" {
            let root = rest.split_whitespace().next().unwrap_or("");
            rest = rest[root.len()..].trim_start();
        }
        if rest.is_empty() {
            return Tier::T2;
        }
        return classify(rest).max(Tier::T2);
    }

    // --- T3: critical. Checked first; nothing below may downgrade these. ---
    // Privilege escalation: sudo/doas/pkexec anywhere, su/gosu as a real
    // (normalized) first token. Over-classifying here is the safe direction.
    if lower.contains("sudo")
        || lower.contains("doas")
        || lower.contains("pkexec")
        || matches!(first, "su" | "gosu" | "runas")
    {
        return Tier::T3;
    }
    if lower.contains(":(){") || lower.contains("mkfs") || lower.starts_with("dd ") {
        return Tier::T3;
    }
    // Recursive rm in any flag spelling: -r, -R, bundles (-rfv), long form.
    // The gate uses the resolved first token, so `command rm` is caught.
    let resolved = strip_wrappers(core);
    let toks: Vec<&str> = resolved.split_whitespace().collect();
    if first == "rm" {
        let recursive = toks.iter().skip(1).any(|t| {
            *t == "--recursive"
                || (t.starts_with('-')
                    && !t.starts_with("--")
                    && t[1..].chars().any(|c| c == 'r' || c == 'R'))
        });
        if recursive {
            let targets: Vec<&&str> = toks
                .iter()
                .skip(1)
                .filter(|t| !t.starts_with('-'))
                .collect();
            if targets.iter().any(|t| is_mass_target(t)) || targets.is_empty() {
                return Tier::T3;
            }
            return Tier::T2;
        }
        return Tier::T1;
    }
    if cmd.split_whitespace().any(is_secret_token) {
        return Tier::T3;
    }
    if first == "env" || first == "printenv" {
        return Tier::T3;
    }
    if first == "kubectl" {
        return kubectl_tier(core, &lower);
    }

    // --- T2: risky. ---
    // Pipe into privilege escalation is T3: the escalation sink, not a shell.
    // Segments resolve on original case (see `first` above).
    let piped_privesc = core.split('|').skip(1).any(|seg| {
        matches!(
            seg_first(seg),
            "sudo" | "doas" | "su" | "gosu" | "runas" | "pkexec"
        )
    });
    if piped_privesc {
        return Tier::T3;
    }
    // Pipe-to-shell in any spacing: split on '|' and inspect each segment's
    // (normalized) command, so `|sh`, `|  sh`, `|/bin/sh` all match while
    // `|show` does not.
    let piped_shell = core
        .split('|')
        .skip(1)
        .any(|seg| matches!(seg_first(seg), "sh" | "bash" | "dash" | "zsh" | "fish"));
    if piped_shell {
        return Tier::T2;
    }
    if first == "find" {
        let args: Vec<&str> = core.split_whitespace().skip(1).collect();
        if args.contains(&"-delete") {
            // "." is cwd-scoped (like `rm -R ./target`): risky, not critical.
            let mass_path = args
                .iter()
                .filter(|a| !a.starts_with('-') && !["{}", "+", ";"].contains(a))
                .any(|p| is_mass_target(p) && *p != "." && *p != "./");
            if mass_path {
                return Tier::T3;
            }
            return Tier::T2;
        }
        if args
            .iter()
            .any(|a| matches!(*a, "-exec" | "-execdir" | "-ok" | "-okdir"))
        {
            return Tier::T2;
        }
    }
    // Hidden commands: substitution, process substitution both directions,
    // eval, and `sh -c '...'` all run text the user can't see at a glance.
    if lower.contains("$(")
        || lower.contains('`')
        || lower.contains("<(")
        || lower.contains(">(")
        || first == "eval"
        || (matches!(first, "sh" | "bash" | "dash" | "zsh" | "fish")
            && core
                .split_whitespace()
                .skip(1)
                .any(|a| a == "-c" || a.starts_with("-c")))
    {
        return Tier::T2;
    }
    if first == "docker" {
        let sub = core.split_whitespace().nth(1).unwrap_or("").to_lowercase();
        return match sub.as_str() {
            "ps" | "images" | "ls" => Tier::T0,
            _ => Tier::T2,
        };
    }
    if first == "git" {
        let t = git_tier(core);
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
        return git_tier(core);
    }
    if matches!(
        first,
        "npm" | "cargo" | "pip" | "mkdir" | "touch" | "cp" | "mv" | "sed" | "tee" | "curl" | "wget"
    ) {
        return Tier::T1;
    }
    // Note: `rm` is handled above (recursive spellings); plain rm returns T1 there.

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
