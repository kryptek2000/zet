# A2 — SSH transport in `zet-pty` (design)

Status: proposed · Owner: architect · Task: `t_636d4d00` · Base commit: `ce5155b`

## Decision

**SSH transport lives in `zet-pty` as a second variant of a `Session` enum, backed by `puressh`'s
blocking, pure-Rust frontend. `zet-pty` stays std-only on the terminal hot path — no tokio, no C
library, no system binary — and the window keeps `Option<Session>` calling the same methods.
`child_id` and `erase_char` are honored as `None` / `0x7f` for remote shells. Only the interactive
shell transport ships in this milestone; the remote daemon, context pack, agent-near-code and
mosh-like reconnect of spec §8:169 are deferred because `crates/zet-daemon/src/lib.rs` is still a
one-line stub and needs its own protocol.**

## Context

- Spec §8:144 — `zet-pty — portable PTY (fork/exec, Win ConPTY), SSH transport` (closed crate set;
  §8:139–153, Appendix B:238). No new crate.
- Spec §8:155 — "PTY output never blocks on model I/O."
- Spec §8:169 — SSH story: `zet ssh host` ships `zet-daemon` + context pack remotely; agent runs
  near the code, UI streams blocks (mosh-like reconnect).
- Spec §9:173 — macOS + Linux first; Windows ConPTY deferred (§14:218).
- Spec §10:175–177 — privacy: secrets (`~/.aws`, `~/.ssh`, env) never leave the machine.
- Spec §13:208 — SSH remote agent is M1.
- Spec §14:218 — Windows ConPTY parity deferred.
- AGENTS.md — perf gates are release gates; PTY output must never block on model I/O; tiered
  approvals T0–T3; no mutation without a recorded checkpoint.
- `crates/zet-pty/Cargo.toml:7-12` — current deps: `portable-pty 0.9`, `zet-term`, `nix 0.28`
  (unix only). **No async runtime.**
- `crates/zet-pty/src/lib.rs:41-134` — `Session` API: `spawn`, `write_all`, `clone_reader`,
  `wait_with_output`, `try_exit`, `shutdown`, `child_id`, `erase_char` (unix), `resize`.
- `crates/zet/src/window/mod.rs:64` — `session: Option<Session>`.
- `crates/zet/src/window/mod.rs:104-136` — reader thread: `std::thread::spawn` over
  `clone_reader()` + `mpsc::channel::<Vec<u8>>()`, waking the event loop per chunk.
- `crates/zet/src/window/mod.rs:279-289` — `shell_cwd`: `/proc/{pid}/cwd` readlink, unix only.
- `crates/zet/src/window/mod.rs:841-849` — `fit_all`: `term.resize` then `session.resize`.
- `crates/zet/src/window/mod.rs:1594-1599` — Backspace rewritten to `tab.erase`.
- `crates/zet/src/main.rs:30-60` + `crates/zet-pty/src/lib.rs:493-507` — `CLI_SUBCOMMANDS` is the
  shared group list; adding a group touches both (comment at `main.rs:56`).
- `crates/zet-daemon/src/lib.rs:1` — one-line stub.
- Research: `~/wiki/comparisons/ssh-client-options.md` (card `t_5d17ce49`), verdict deferred to
  the Architect.

## Design

### 1. Library choice — `puressh` 0.2.0

**Pick: `puressh` 0.2.0** (pure Rust, blocking std frontend). Reasoning against the constraints:

| Constraint | `puressh` | `russh` 0.64.1 | `ssh2` 0.9.6 | `openssh` 0.11.6 |
|---|---|---|---|---|
| Async runtime added to `zet-pty` | **none** (default build std-only) | mandatory tokio + futures | none | mandatory tokio |
| System binary at runtime | none | none | none | `/usr/bin/ssh` |
| C library at build time | **none** (crypto via `purecrypto`) | aws-lc-rs/ring (C/asm) | libssh2 + OpenSSL | none |
| PTY request API | `SharedClient::shell_stream(term, cols, rows, …)` | `Channel::request_pty` | `Channel::request_pty` | **none** (`-T` always) |
| Window-change | `send_window_change` (RFC 4254 §6.7) | `Channel::window_change` | `request_pty_size` | none |
| Exit status | `exit_status()` / `wait_exit_status` | `ChannelMsg::ExitStatus` | `Channel::exit_status` | ambiguous `ssh` exit |
| Host-key policy ships in crate | `HostKeyPolicy` (fails closed) | storage only | storage only | `StrictHostKeyChecking` |
| Read/write concurrency | multi-channel, splittable reader | async tasks | **one Mutex per Session** | n/a |

`openssh` is disqualified first: it has no PTY API at all and passes `-T` for every command
(research §"openssh", `src/process_impl/session.rs`), so it cannot satisfy the requirement without
forking upstream. `russh` and `openssh` both force a mandatory tokio dependency onto `zet-pty`,
which today is std-only and sits on the terminal read path (`crates/zet/src/window/mod.rs:97-113`);
that is a large, risky change for a crate whose perf budget is a release gate. `ssh2` avoids a
runtime but pulls libssh2 + OpenSSL at build time and serialises every operation on one `Session`
behind a `Mutex` — a blocking read blocks writes, which is exactly the head-of-line behaviour we
must not introduce on the PTY path.

`puressh` is the only candidate that is simultaneously pure Rust, std-only by default, blocking,
multi-channel, and ships a host-key policy that fails closed. Its costs are real and are accepted
with mitigations:

- **Pre-1.0** ("functional, pre-1.0", public API may change before 1.0) → pin the exact version
  (`=0.2.0`) in `Cargo.toml`; the `Transport` trait (§2) keeps the swap cost to one adapter file.
- **Unaudited, single maintainer** (KarpelesLab, 6,250 all-time downloads) → treat as a
  supply-chain risk: vendor review in S3 of the migration, and keep the crypto/backend choice
  auditable (purecrypto, no FFI). If an audit fails, fall back to `ssh2` behind the same
  `Transport` trait; that fallback is the documented escape hatch.
- **MSRV 1.95, edition 2024** → clears the project's Rust 1.99.1 toolchain.

Cargo.toml change (the only dependency change, in `crates/zet-pty`):

```toml
[dependencies]
portable-pty = "0.9"
zet-term = { path = "../zet-term" }

[dependencies.puressh]
version = "=0.2.0"
optional = true
default-features = false
features = ["blocking"]   # std-only frontend; no tokio/mio

[target.'cfg(unix)'.dependencies]
nix = { version = "0.28", features = ["term"] }

[features]
default = ["ssh"]
ssh = ["dep:puressh"]
```

`zet-pty` must not gain an edge to `zet-agent`, `zet-models` or `zet-policy` (same separation A1
enforced for `zet-gpu`); `puressh` has no such edge.

### 2. Interface shape — `Session` becomes an enum

Option (a) a sibling `SshSession` would force every call site to branch on a type. Option (c) a
`Transport` trait object in the window (`Option<Box<dyn Transport>>`) adds vtable dispatch on the
hot path and churns `Tab`. **Chosen: (b) `Session` becomes an enum over local vs. remote**, with a
`Transport` trait that both variants satisfy for documentation and testing. The window keeps
`Option<Session>` and every existing call compiles unchanged because the public methods keep their
signatures and delegate.

Honesty table for the eight `Session` methods on a remote shell:

| Method | Local | Remote | Notes |
|---|---|---|---|
| `write_all` | yes | yes | bytes to remote stdin |
| `clone_reader` | yes | yes (splittable reader) | remote stdout/stderr |
| `resize` | yes | yes | sends window-change (§5) |
| `try_exit` | yes | yes | polls remote exit status |
| `shutdown` | yes | yes | close channel / EOF, then drop |
| `child_id` | `Some(pid)` | **`None`** | no local PID; `/proc` lookups cannot run |
| `erase_char` | termios VERASE | **`0x7f`** | remote line discipline's own erase char |
| `wait_with_output` | yes | yes | blocks until channel EOF |

`child_id() == None` is the single signal the window uses to disable `/proc`-based cwd and
local-only features. `erase_char() == 0x7f` is the standard remote default; the window's Backspace
rewrite (`mod.rs:1596`) is a no-op in that case.

New/changed public API in `crates/zet-pty/src/lib.rs`:

```rust
pub mod shell;
#[cfg(feature = "ssh")]
pub mod ssh;

/// The shared contract. `Session` implements it; tests may implement it for
/// fakes. Object-safe.
pub trait Transport: Send {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn clone_reader(&self) -> std::io::Result<Box<dyn std::io::Read + Send>>;
    fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()>;
    fn try_exit(&mut self) -> std::io::Result<Option<u32>>;
    fn shutdown(&mut self);
    fn child_id(&self) -> Option<u32>;
    fn erase_char(&self) -> u8;
    fn is_remote(&self) -> bool { false }
}

pub struct LocalSession {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn std::io::Write + Send>,
    child:  Box<dyn portable_pty::Child + Send + Sync>,
}

#[cfg(feature = "ssh")]
pub use ssh::SshSession;

pub enum Session {
    Local(LocalSession),
    #[cfg(feature = "ssh")]
    Ssh(SshSession),
}

impl Session {
    // unchanged signature
    pub fn spawn(program: &str, args: &[&str], cols: u16, rows: u16)
        -> std::io::Result<Self>;

    #[cfg(feature = "ssh")]
    pub fn spawn_ssh(target: &SshTarget, cols: u16, rows: u16, opts: &SshOptions)
        -> std::io::Result<Self>;

    // all eight methods delegate to the variant
    pub fn is_remote(&self) -> bool;
}

impl Transport for Session { /* delegates */ }
```

SSH-specific types (in `crates/zet-pty/src/ssh.rs`):

```rust
#[derive(Clone, Debug)]
pub struct SshTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: u16,            // default 22
}

impl SshTarget {
    /// `host`, `user@host`, `host:port`, `user@host:port`, `[ipv6]:port`.
    pub fn parse(s: &str) -> std::io::Result<Self>;
}

#[derive(Clone, Debug)]
pub enum HostKeyPolicy {
    Strict,      // unknown OR changed → fail (default)
    AcceptNew,   // unknown → record + proceed; changed → fail
    // AcceptAny is intentionally not exposed by default; see §4.
}

#[derive(Clone, Debug)]
pub struct SshOptions {
    pub host_key: HostKeyPolicy,        // default Strict
    pub identity_files: Vec<std::path::PathBuf>, // default ~/.ssh/id_ed25519, id_rsa, …
    pub use_agent: bool,                // default true (SSH_AUTH_SOCK)
    pub term: String,                   // default "xterm-256color"
    pub known_hosts: Option<std::path::PathBuf>, // default ~/.ssh/known_hosts
}

pub struct SshSession { /* puressh client + split reader/writer + cached exit status */ }

impl SshSession {
    pub fn connect(target: &SshTarget, opts: &SshOptions) -> std::io::Result<Self>;
}
```

`Session::spawn_ssh` is `SshSession::connect` followed by `shell_stream(term, cols, rows, …)` and
wrapping; it blocks until the handshake + auth + shell request complete. `SshSession` stores the
puressh `SharedClient` (for `send_window_change` and exit status) plus a writer and a reader handle
obtained from puressh's split API. If puressh 0.2.0's stream turns out not to split cleanly, the
implementor falls back to one `Arc<Mutex<OwnedChannelStream>>` shared by reader and writer —
serialised, but still off the UI thread, no worse than `ssh2` and only on the remote path.

### 3. Threading topology

Identical to the existing local pattern at `crates/zet/src/window/mod.rs:118-136`:

```
Tab::spawn (UI thread)
  ├── Session::spawn / spawn_ssh   (blocking constructor, runs once per new tab)
  ├── session.clone_reader()       -> Box<dyn Read + Send>
  ├── mpsc::channel::<Vec<u8>>()
  └── std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop { match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => { let _ = tx.send(buf[..n].to_vec()); let _ = wake.send_event(()); }
        } }
      })
```

- No tokio runtime anywhere in `zet-pty` or the window. No model I/O on this path; the reader
  thread only forwards bytes and wakes the event loop.
- PTY parse (`term.feed`) stays on the UI thread but is bounded by the same 4 KiB chunk loop
  already used for local shells; perf gate `cat 100k lines < 0.9s` (§7) is unchanged because the
  remote path uses the same chunking.
- For very slow handshakes, `spawn_ssh` blocks the UI thread while the tab is being created. The
  implementor may move construction into a `std::thread` and deliver the `Session` through a channel
  into a tab that starts as `session: None`; `Tab` already models `Option<Session>`. That is an
  optional S5 enhancement, not a requirement for the first ship.

### 4. Host-key policy and credentials

- **known_hosts file:** `~/.ssh/known_hosts` by default (`SshOptions::known_hosts` overrides;
  honouring `UserKnownHostsFile` is deferred). Hashed `|1|…` entries and `[host]:port` are handled
  by puressh's parser.
- **Policy default: `Strict`.** Unknown host → print the server's fingerprint to `/dev/tty` and
  prompt `accept? (y/N)`; on `y`, append the key (TOFU for that host only). Changed key → **hard
  fail**, no override flag in M1; the user must remove the stale entry from `known_hosts` by hand.
  `AcceptNew` is available for non-interactive use (CI/recipes) and auto-records unknown hosts.
  `AcceptAny` is deliberately not exposed in the public API: it defeats the point and is a T3-class
  risk.
- **Credentials, in order:** (1) ssh-agent via `SSH_AUTH_SOCK` (`use_agent = true` default);
  (2) unencrypted identity files from `~/.ssh/` (`id_ed25519`, `id_ecdsa`, `id_rsa`); (3) if a
  key is passphrase-protected, prompt on `/dev/tty` with echo disabled (reusing the `NoEcho`
  pattern from `crates/zet/src/main.rs:198-239`); (4) password / keyboard-interactive only if the
  user opts in via `SshOptions` and a TTY is present. Failures are loud and name the missing piece.
- **Secrets never leave the machine** (spec §10:175-177, §6.4): keys, passphrases and
  `known_hosts` are read locally by `zet-pty`; only the SSH wire traffic (which is the user's
  terminal bytes) crosses the network. No key material is logged, sent to a model, or stored in
  `~/.zet`. The redactor's deny-list already covers `~/.ssh`.
- **Host-key mismatch behaviour:** puressh's `host_key_mismatched()` refuses to send passwords,
  keyboard-interactive credentials, agent forwarding, X11 and port forwards — matching OpenSSH's
  `StrictHostKeyChecking` semantics but safer. ZET propagates that as a hard error naming the
  mismatch.

### 5. Window resize

`fit_all` (`crates/zet/src/window/mod.rs:841-849`) already calls `session.resize(cols, rows)` after
resizing the grid. For a remote session that maps to puressh's `send_window_change(cols, rows, px_w,
px_h)` (RFC 4254 §6.7). The local grid always resizes first, so a remote that ignores or rejects
window-change still renders correctly (just without reflow). Errors from `resize` are ignored with
a `debug!`-level note; they must never panic the UI thread or block on I/O.

### 6. Exit status

- `SshSession::try_exit` calls puressh's non-blocking `exit_status()` and returns `Some(code)` once
  the server has sent `exit-status` (and caches it). `wait_with_output` blocks until channel EOF
  and then returns the code.
- The window's `pump_tab` (`mod.rs:336-353`) calls `try_exit` to set `tab.dead`, identical to
  local shells.
- **Block recording** is marker-driven (OSC 133/633 `D;code` in `zet-blocks/src/recorder.rs`).
  A remote shell emits those markers only if the remote side runs zet's shell integration, which
  M1 does not install. Therefore remote commands are not recorded as blocks in M1; the tab still
  dies correctly via `try_exit`. Transport exposes `exit_status()` so a future recorder fallback
  (and the M2 daemon) can record remote blocks honestly. This is stated as a limit, not hidden.

### 7. Module layout (no new crate)

```
crates/zet-pty/
  Cargo.toml            (+ puressh optional, feature "ssh")
  src/
    lib.rs              Session enum + Transport trait + LocalSession (moved fields)
    shell.rs            unchanged
    ssh.rs              (new, cfg(feature = "ssh")) SshTarget, SshOptions, HostKeyPolicy,
                        SshSession
```

`crates/zet-pty/src/lib.rs` keeps `pub mod shell;` and gains `#[cfg(feature = "ssh")] pub mod ssh;`.
`syntax_ok`, `first_word`, `KnownCommands`, `zet_fallback` and `CLI_SUBCOMMANDS` are untouched.

### 8. Window integration (small, surgical)

- `crates/zet/src/window/mod.rs:64` stays `session: Option<Session>`.
- `Tab::spawn` gains a parameter `ssh: Option<SshTarget>` (or a separate `Tab::spawn_ssh`). When
  `Some`, it calls `Session::spawn_ssh(&target, 80, 24, &SshOptions::default())`; otherwise the
  existing `Session::spawn("env", …)`. `erase` is read from the session as today (`mod.rs:145`),
  which yields `0x7f` for remote.
- `shell_cwd` (`mod.rs:279-289`) is updated to prefer `shell_state.cwd()` (OSC 633 `P;Cwd=`) and
  to run the `/proc/{pid}/cwd` readlink **only when `session.as_ref().map(|s| s.is_remote()) ==
  Some(false)`**. This makes remote cwd work when the remote shell emits the marker, and avoids a
  bogus `/proc` lookup for a remote PID. `git_branch` in the recorder runs `git -C` locally and
  will simply return `""` for a remote cwd; acceptable in M1.
- `fit_all` (`mod.rs:841-849`) needs no change: it already calls `session.resize` for every tab.
- New tabs created via the tab-bar shortcut use the local path (no SSH target).

### 9. CLI entry point — `zet ssh`

- Add `"ssh"` to `crates/zet-pty/src/lib.rs:493` `CLI_SUBCOMMANDS` and a match arm in
  `crates/zet/src/main.rs:30` `cli_dispatch`: `["ssh", rest @ ..] => ssh_cmd(rest)`.
- `ssh_cmd` parses `[user@]host[:port]` (and optional `--port`, `--identity`), then opens the
  window with the first tab as an SSH session: `zet::window::run(None, Some(target))`.
- `window::run` signature becomes `run(goal: Option<String>, ssh: Option<SshTarget>) -> io::Result<()>`;
  bare `zet` and `zet <goal>` pass `None`. `App` gains `pending_ssh: Option<SshTarget>` consumed in
  `about_to_wait` the same way `pending_goal` is (A1 §5), creating the SSH tab once the event loop
  is live.
- Help text in `main.rs` gains a `zet ssh [user@]host[:port]` line.
- `zet ssh` with no host → usage error, exit 2 (consistent with `crates/zet/tests/cli.rs`).

### 10. Scope boundary — in vs. deferred

**In this milestone:**
1. SSH transport in `zet-pty` (Session enum + `SshSession` via puressh) + `Transport` trait.
2. `zet ssh <host>` entry point opening a window whose first tab is an interactive remote shell.
3. Host-key verification (strict default, accept-new on prompt), ssh-agent + `~/.ssh` keys, resize,
   exit status, reader thread off the UI thread.

**Deferred (and why):**
- **Shipping `zet-daemon` remotely, context pack, agent running near the code, mosh-like reconnect**
  (spec §8:169) — `crates/zet-daemon/src/lib.rs` is a one-line stub and this needs a wire
  protocol, remote process supervision and reconnect state; it is a separate design (call it A3)
  and a separate milestone. M1's `zet ssh` gives an interactive shell only.
- **Background/delegated runs over SSH** — depends on the remote daemon above.
- **Remote block recording without OSC 133 markers** — needs a recorder fallback to `exit_status()`;
  deferred to keep the transport change minimal. Remote shells that emit the markers already record.
- **Remote command execution (`zet ssh host cmd…`)** — M1 is interactive shell only.
- **Passphrase-protected keys polish / password auth in a GUI prompt** — M1 prompts on `/dev/tty`;
  a graphical prompt is later.
- **Windows** — already deferred per spec §14:218; puressh is unix-first and the `ssh` feature
  stays off on Windows until then.

## Risks / rejected alternatives

- **`russh` 0.64.1 — rejected.** Mandatory tokio + futures on `zet-pty`'s hot path and a crypto
  backend that compiles C/asm (aws-lc-rs or ring). Violates "PTY output must never block on model
  I/O" in spirit (a runtime on the read path) and adds a C toolchain to a crate that today has
  none. Its PTY/resize API is first-class, so it remains the fallback if puressh fails audit.
- **`openssh` 0.11.6 — rejected (hard).** No PTY API at all; upstream passes `-T` for every
  command (`src/process_impl/session.rs`). Cannot satisfy the requirement without forking.
- **`ssh2` 0.9.6 — rejected.** libssh2 + OpenSSL build dependency and one `Mutex` per `Session`
  that serialises reads and writes. Cheaper than russh but still a C toolchain and a
  head-of-line risk on the PTY path. Remains the escape hatch if puressh's audit fails.
- **Trait object in the window (`Option<Box<dyn Transport>>`) — rejected.** Vtable dispatch on
  the byte path and a churn-heavy change to `Tab`; the enum keeps `Option<Session>` and static
  dispatch.
- **New crate for SSH — rejected.** Spec §8:139–153 / Appendix B:238 define the crate set as
  closed; A1 already established that the composition root is `crates/zet`.
- **Tokio in `zet-pty` — rejected** (same reason as russh). The reader thread stays
  `std::thread` + `mpsc`.
- **`AcceptAny` host-key policy — rejected** as a public option. It is a T3-class risk and
  contradicts spec §6.4 / §10.
- **Accepted tradeoffs:** puressh is pre-1.0, unaudited and single-maintainer (6,250 downloads).
  Mitigations: pin `=0.2.0`, keep the `Transport` trait as a swap seam, feature-gate the
  dependency, and document `ssh2` as the fallback. If its stream does not split cleanly, fall back
  to a shared `Mutex` (still off the UI thread). Connection establishment blocks tab creation in
  M1; deferrable to a channel-delivered session (optional enhancement).

## Acceptance checks

Baseline (must hold before and after every step):
```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

**S1 — `Session` becomes an enum, no behaviour change.**
```bash
cargo check -p zet-pty --all-targets
cargo test -p zet-pty
# expect: all existing tests (session.rs, stress.rs, shell_state.rs) pass unchanged;
#         `Session::spawn`, `child_id`, `erase_char`, `resize` signatures identical.
```

**S2 — puressh wired, feature-gated.**
```bash
cargo check -p zet-pty --features ssh
cargo check -p zet-pty --no-default-features
# expect: both green; `cargo tree -p zet-pty | grep -i puressh` only when ssh enabled.
```

**S3 — SSH transport compiles and a real host connects.**
```bash
ZET_SSH_TEST_HOST=user@host cargo test -p zet-pty --features ssh -- --ignored
# expect: handshake + shell request succeed; reader thread forwards bytes;
#         resize and exit status observed; unknown host prints fingerprint and prompts.
```

**S4 — window integration.**
```bash
cargo check -p zet --all-targets
# expect: green; `fit_all` resizes a remote tab without panic; `shell_cwd` returns "" for a
#         remote session without a marker and the OSC 633 Cwd value when the remote shell emits it.
```

**S5 — `zet ssh` end to end.**
```bash
cargo build -p zet && ./target/debug/zet ssh user@host
# expect: window opens, first tab is a remote shell, typing works, window resize reflows,
#         Ctrl+D / `exit` closes the tab and the process exits 0.
cargo run -p zet -- ssh                # expect: exit 2, usage on stderr
cargo run -p zet -- bogus              # expect: exit 2 (unchanged, cli.rs:20)
```

**S6 — host-key failure modes.**
```bash
# changed key: expect hard error naming the mismatch, non-zero exit, no prompt.
# unknown host non-interactive (no TTY): expect failure, not silent accept.
```

**S7 — perf gates unchanged for the local path.**
```bash
ZET_PERF=1 ./target/release/zet 2>&1 | grep 'mark shell_spawn'
# expect: same ~40 ms as today; remote path is off the measured local startup.
```