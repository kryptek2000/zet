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

/// Interpreters and script runners whose arguments are code, not data:
/// `python3 -c '...'`, `perl -e`, `bash payload.sh`, `node -e`. The body
/// is arbitrary and unreadable at a glance, exactly like `sh -c`, so it
/// floors at T2. Version suffixes match (`python3.12`, `ruby3.3`).
fn runs_undisclosed_code(first: &str, core: &str) -> bool {
    let name: String = first
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect();
    if !matches!(
        name.as_str(),
        // Shells: a script file is code too, not just `-c`.
        "sh" | "bash" | "dash" | "zsh" | "ksh" | "fish"
            // Interpreters.
            | "python" | "perl" | "ruby" | "node" | "php" | "lua" | "deno" | "bun"
            | "osascript" | "irb" | "wish" | "tclsh" | "expect"
    ) {
        return false;
    }
    let args: Vec<&str> = core.split_whitespace().skip(1).collect();
    // A bare interpreter opens a REPL: code without a command line.
    if args.is_empty() {
        return true;
    }
    // Inline-code flags (attached or not), program on stdin, or any plain
    // argument: a script path is undisclosed code with no flag to mark it.
    args.iter()
        .any(|a| !a.starts_with('-') || is_inline_code_flag(a))
}

/// Inline-code flags, in every spelling the wild uses: `-c`, and the
/// attached form `-c'payload'` that is one shell token. Attached short
/// flags are not exotic — they are how people write them by hand.
fn is_inline_code_flag(arg: &str) -> bool {
    matches!(arg, "-" | "--eval" | "--command")
        || arg.starts_with("-c")
        || arg.starts_with("-e")
        || arg.starts_with("-E")
        || arg.starts_with("--eval=")
        || arg.starts_with("--command=")
}

/// Key prefixes that identify a credential on sight. Matches the scrubber
/// in zet-blocks, so the two agree on what a secret looks like: a command
/// carrying one is T3 whether or not the token looks like a path.
const SECRET_KEY_PREFIXES: &[&str] = &[
    "github_pat_",
    "sk-or-",
    "sk-ant-",
    "sk-proj-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxo-",
    "ghp_",
    "gho_",
    "AKIA",
    "sk-",
];

/// Minimum tail after a key prefix before it counts as a credential
/// (`sk-folders` and other short fragments are not keys). Mirrors the
/// per-prefix minimums in `zet_blocks::SECRET_PREFIXES`; the two must
/// agree, or the store redacts text the gate let through.
/// `crates/zet-blocks/tests/gate_scrub_agreement.rs` checks that for real.
pub const SECRET_KEY_MIN_TAIL: usize = 16;

/// `Bearer ` carries its own, lower floor (8), exactly as the scrubber
/// does: an 8-character bearer token is redacted from the transcript by
/// zet-blocks, so it must not sail through the gate at T1.
pub const BEARER_MIN_TAIL: usize = 8;

/// Length of the credential-looking tail at the start of `s`. Same
/// character set the scrubber uses (`is_token_char` in zet-blocks), so
/// the two measure a tail identically — quotes are not token characters
/// here either, and `unquote` is what looks past them.
fn key_tail_len(s: &str) -> usize {
    s.chars()
        .take_while(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '~' | '+' | '/' | '=' | '.')
        })
        .count()
}

/// Peels every layer of surrounding quotes. A key reaches the command
/// line quoted more often than not (`export K='sk-…'`), and quoted twice
/// when the author hedged between quote styles (`"'sk-…'"`), while the
/// scrubber matches it positionally regardless — so the gate has to look
/// past all of them or the two disagree about what a secret is.
///
/// A layer is stripped only when it actually pairs: `'sk-…"` (mismatched,
/// and a shell syntax error besides) comes back untouched. That is the
/// point — unquoting must never manufacture a key match out of text that
/// was not one.
fn unquote(tok: &str) -> &str {
    let mut t = tok;
    loop {
        let Some(first) = t.chars().next() else {
            return t;
        };
        if !matches!(first, '\'' | '"') || t.len() < 2 {
            return t;
        }
        // Byte slicing is safe: both ends are ASCII quotes.
        if t.as_bytes()[t.len() - 1] != first as u8 {
            return t;
        }
        t = &t[1..t.len() - 1];
    }
}

/// True when a token is a credential on sight: a known key prefix with a
/// real-looking tail, looking past surrounding quotes.
fn looks_like_key(tok: &str) -> bool {
    let tok = unquote(tok);
    SECRET_KEY_PREFIXES.iter().any(|prefix| {
        tok.strip_prefix(prefix)
            .is_some_and(|tail| key_tail_len(tail) >= SECRET_KEY_MIN_TAIL)
    })
}

/// `Authorization: Bearer <key>` does not survive shell word-splitting:
/// the header name and the value are separate tokens, so this check needs
/// the whole line. Same shape as the position-independent `sudo` rule.
fn carries_bearer_key(cmd: &str) -> bool {
    const HEADER: &str = "Bearer ";
    let mut rest = cmd;
    while let Some(at) = rest.find(HEADER) {
        let tail = &rest[at + HEADER.len()..];
        if key_tail_len(unquote(tail)) >= BEARER_MIN_TAIL {
            return true;
        }
        rest = tail;
    }
    false
}

/// Path-like tokens that smell like secrets. Requires a `/`, `.`, or `~`
/// so commit messages like "secret fix" don't false-positive.
fn is_secret_token(tok: &str) -> bool {
    // Key prefixes identify themselves wherever they appear — checked on
    // the whole token and on the value half of `VAR=value`, which is how
    // a key actually reaches the shell (`export K=sk-or-v1-…`).
    let value = tok.split_once('=').map(|(_, v)| v).unwrap_or("");
    if looks_like_key(tok) || (!value.is_empty() && looks_like_key(value)) {
        return true;
    }
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
    if cmd.split_whitespace().any(is_secret_token) || carries_bearer_key(cmd) {
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
    // Interpreters and script runners: everything after the binary is
    // code the gate cannot read. Found by the red-team suite, which
    // showed `python3 -c 'shutil.rmtree("/")'` falling to T1 — which
    // AUTO-APPROVES inside a repo, i.e. an unprompted mass delete. T2 is
    // the same floor `sh -c` and `find -exec` already get.
    if runs_undisclosed_code(first, core) {
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
