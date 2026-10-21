# A1 — zet binary / window unification (design)

Status: proposed · Owner: architect · Task: `t_bbb01ff7` · Base commit: `30ac02b`

## Decision

**The window loop moves into `crates/zet` (the existing `zet` binary crate) as
`crates/zet/src/window/`. `zet-gpu` stays a pure render crate. No new crate.**

`crates/zet` becomes lib + bin: `src/lib.rs` (`pub mod mode; pub mod window;`) +
`src/main.rs` (argv → mode → CLI dispatch | window). `crates/zet-gpu/examples/shell.rs`
is deleted; its 1814 lines move to `crates/zet/src/window/mod.rs` and `zet-gpu` drops
all six dev-dependency edges.

This is **Option 3**, and it is Option 2's separation without Option 2's crate:
`zet-gpu` ends up with zero upward edges either way, and the extra crate's only
consumer would be this one binary.

## Context

- Spec §8:139–153 lists the crates as a closed set; Appendix B:238 repeats it. There is
  no `zet-app`.
- Spec §8:142 — `zet — binary, CLI (zet, zet run, zet replay, zet undo)`; Appendix
  A:223 — `zet # open window`. **The spec already makes the `zet` binary the window entry
  point.** Appendix A:224 — `zet "fix failing e2e in ./api"` — the same binary takes a goal.
- `crates/zet-gpu/Cargo.toml` — the window's logic crate edges (`zet-agent`, `zet-blocks`,
  `zet-context`, `zet-models`, `zet-policy`, `zet-suggest`) plus `arboard`, `notify-rust`
  are `[dev-dependencies]`, and **only** `examples/shell.rs` uses any of them (verified by
  grep across `examples/`, `tests/`, `src/`). `examples/window.rs:27` and `examples/text.rs:45`
  use `pollster` only.
- Cargo does not propagate `dev-dependencies` to dependents. So `crates/zet` cannot call
  `zet_gpu::shell::run()` while those edges stay dev-only — Option 1 does not fail because of
  taste, it fails to build.
- `cargo tree -p zet-gpu --edges normal,dev` today puts, under a *renderer*:
  `zet-agent → zet-models → {keyring, ureq}` and `zet-agent → zet-context → zet-blocks →
  rusqlite (bundled C SQLite)`, plus `notify-rust`.
- `docs/SHORTCUTS.md:3–4` — the shipped launcher target is documented as
  `cargo run -p zet-gpu --example shell`. That line is the bug this task fixes.
- `crates/zet/tests/cli.rs:20` asserts `zet bogus` **exits non-zero**. Any mode rule that
  treats an unknown single word as a goal breaks it. `cli.rs:274` iterates
  `zet_pty::CLI_SUBCOMMANDS` and asserts exit 2 for every group.
- `crates/zet/src/main.rs:31` — `["__run_worker", id]` is a CLI arm **not** present in
  `CLI_SUBCOMMANDS` (`crates/zet-pty/src/lib.rs:493`). A naive mode rule would send the
  daemon's worker into a window.
- Baseline, measured on this machine before any change: `cargo fmt --all -- --check`,
  `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` — all exit 0 (`zet`: 4 unit + 37 `cli.rs` tests).
- `crates/zet-gpu/examples/shell.rs:86–90` spawns the shell as
  `env TERM=xterm-256color <shell> -i` and then *writes* `source {hook}\nclear\n` into the
  PTY. The hook channel is therefore already open — no `zet-pty` API change is needed to
  export a session id.

## Design

### 1. Where the loop lives, and the edges

```
crates/zet  (lib + bin, composition root — the only crate that may depend on everything)
  -> zet-gpu, zet-agent, zet-models, zet-blocks, zet-pty, zet-context,
     zet-policy, zet-suggest, winit, pollster, arboard, notify-rust
crates/zet-gpu  (pure renderer)
  -> zet-term, zet-pty
  dev -> pollster
```

Layering rule this encodes, to be written into `AGENTS.md`:

> Leaf/render crates (`zet-term`, `zet-gpu`, `zet-policy`) never depend upward on
> orchestration crates (`zet-models`, `zet-context`, `zet-blocks`, `zet-agent`).
> `crates/zet` is the composition root and is the only crate allowed to depend on all of them.

### 2. Exact `Cargo.toml` changes

`crates/zet/Cargo.toml`:

```toml
[lib]
name = "zet"
path = "src/lib.rs"

[[bin]]
name = "zet"
path = "src/main.rs"

[dependencies]
zet-gpu     = { path = "../zet-gpu" }      # NEW edge
zet-agent   = { path = "../zet-agent" }
zet-models  = { path = "../zet-models" }
zet-blocks  = { path = "../zet-blocks" }
zet-pty     = { path = "../zet-pty" }
zet-context = { path = "../zet-context" }
zet-policy  = { path = "../zet-policy" }   # PROMOTED from [dev-dependencies]
zet-suggest = { path = "../zet-suggest" }  # NEW edge
winit       = "0.30"                       # NEW (version matches zet-gpu)
pollster    = "1"                          # NEW (block_on(Renderer::new(..)))
arboard     = "3"                          # NEW (clipboard)
notify-rust = "4"
serde_json  = "1"

[dev-dependencies]
# zet-policy moved up; nothing else left.

[target.'cfg(unix)'.dependencies]
nix = { version = "0.28", features = ["term", "signal", "fs"] }
```

`crates/zet-gpu/Cargo.toml`:

```toml
[dependencies]
wgpu        = "30"
winit       = "0.30"
cosmic-text = "0.19"
bytemuck    = { version = "1", features = ["derive"] }
zet-term    = { path = "../zet-term" }
zet-pty     = { path = "../zet-pty" }

[dev-dependencies]
pollster = "1"
# REMOVED with examples/shell.rs: arboard, notify-rust, zet-agent, zet-blocks,
# zet-context, zet-models, zet-policy, zet-suggest.
```

No other `Cargo.toml` changes. No new workspace member. `Cargo.lock` changes only by
pruning zet-gpu's dev edges.

### 3. File layout

```
crates/zet/src/
  lib.rs        pub mod mode; pub mod window;
  main.rs       argv -> mode::plan -> window::run | cli_dispatch (today's match, verbatim)
  mode.rs       Mode, Env, plan()  — pure, unit-tested, no GUI
  window/
    mod.rs      pub fn run(goal: Option<String>) -> io::Result<()>; App;
                impl ApplicationHandler (event loop, about_to_wait, window_event)
    tab.rs      Tab, SearchState, Tab::spawn  (shell.rs:44-166)
    agent.rs    PendingExec, PtyExec, AgentMode, start_agent/start_plan/poll_plan/
                poll_agent_done/poll_undo_done  (shell.rs:200-254, 370-499, 971-1181)
    input.rs    key/mouse handling, NL request, route_direct, resolve_spec,
                update_ghost  (shell.rs:499-576, 643-760, 1288-1765)
    chrome.rs   tab bar, titles, selection, search, copy/paste, fit_all
                (shell.rs:603-642, 658-728, 751-870, 1182-1200)
    models.rs   with_keychains, build_suggester, build_client, perf  (shell.rs:1201-1246)
```

`window/mod.rs` lands **whole** in S2 (mechanical move, compiler-verified); the submodule
split is S5, one file per commit. `main()` (shell.rs:1767–1814) becomes `run(goal)`; the
`App` literal gains `pending_goal: Option<String>`.

`pub fn run(goal)` semantics: `None` → today's behavior exactly. `Some(g)` → store
`pending_goal = Some(g)`; in `about_to_wait`, once the active tab first reaches
`Phase::Prompt` (the `ShellState` phase already tracked at `shell.rs:30`), take it and call
the existing `start_agent(goal, None)` path (`shell.rs:971`). No new agent machinery.

### 4. What happens to the example

- `crates/zet-gpu/examples/shell.rs` — **deleted**. It was the product; it becomes the product.
- `examples/window.rs`, `examples/text.rs` — **kept**; they are pure `Renderer` demos using
  only real deps (`wgpu`, `winit`, `pollster`). They remain the renderer's smoke tests.
- `docs/SHORTCUTS.md:3–4` — launcher target changes from
  `cargo run -p zet-gpu --example shell` to `cargo run -p zet` (installed: `zet`).
- `crates/zet-gpu/tests/*` — untouched; none use the removed dev-deps.

### 5. How `zet` decides between CLI and window

`crates/zet/src/mode.rs`:

```rust
pub enum Mode { Cli, Window { goal: Option<String> }, Usage }

pub struct Env { display: bool, window_allowed: bool }   // Env::detect() reads env vars
pub fn plan(args: &[String], env: &Env) -> Mode;
```

Rules, in order (first match wins):

| # | Condition | Result |
|---|---|---|
| 1 | `args[0]` is `--help`/`-h`/`--version`/`-V` | `Cli` (today's `_` arm → usage, exit 2) |
| 2 | `args[0]` is `--window`/`--gui` | `Window { goal: join(args[1..]) }` — **explicit escape hatch, ignores rule 7** |
| 3 | `CLI_SUBCOMMANDS.contains(args[0])` | `Cli` |
| 4 | `args[0].starts_with("__")` | `Cli` (covers `__run_worker`, `main.rs:31`) |
| 5 | `args.is_empty()` | `Window{None}` if display, else `Usage` |
| 6 | `args.len() >= 2` **or** `args[0]` contains whitespace | `Window{Some(join)}` if display, else `Usage` |
| 7 | otherwise (single word, no whitespace, unknown) | `Usage` (exit 2) |

Rule 6 is what makes Appendix A:224 (`zet "fix failing e2e in ./api"` — one argv element
containing spaces) and `zet fix the failing test` both open a window with a goal. Rule 7 is
what keeps `crates/zet/tests/cli.rs:20` (`zet bogus` must fail) green, and it is honest UX:
a bare unknown word is a typo, a phrase is a goal. `zet --window hello` covers the one-word
goal case.

`Env::display`: `WAYLAND_DISPLAY` or `DISPLAY` on Linux; always true on macOS/Windows.
`Env::window_allowed`: `ZET_NO_WINDOW` unset. When a window is asked for but unavailable,
print `zet: no display; run a subcommand (zet auth|blocks|run|… )` and exit 2 — never panic
on a headless box or in CI.

`main.rs` becomes:

```rust
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match zet::mode::plan(&args, &zet::mode::Env::detect()) {
        zet::mode::Mode::Window { goal } => match zet::window::run(goal) {
            Ok(()) => {}
            Err(e) => { eprintln!("zet: {e}"); std::process::exit(1); }
        },
        zet::mode::Mode::Cli | zet::mode::Mode::Usage => cli_dispatch(&args),
    }
}
fn cli_dispatch(args: &[String]) { /* today's main.rs:8-66, verbatim */ }
```

No existing test changes: every `cli.rs` invocation names a known group (rule 3) or a single
unknown word (rule 7).

### 6. Release binary path

Unchanged and singular: **`target/release/zet`** —
`cargo build --release -p zet` / `cargo install --path crates/zet`. The example path
`target/*/examples/shell` disappears. `crates/zet/tests/cli.rs:5`
(`env!("CARGO_BIN_EXE_zet")`) keeps working: `CARGO_BIN_EXE_<name>` is set for a package's
integration tests whenever that package has a bin target, lib or not.

### 7. One `~/.zet` state root

Today the window and the CLI already land in `~/.zet`, but by **seven independent
hand-rolled path expressions**, and the window invents its session id locally
(`shell.rs:111`, `format!("{secs}-{pid}")`) which no CLI command can name. So `zet replay
<session>` is unusable for a session the window just recorded. That is the real fork: not
the directory, the identity.

- **Owner.** `zet-blocks` gains `pub fn zet_home() -> PathBuf` (`$ZET_HOME` → `$HOME/.zet`
  → `/tmp/.zet`); `Store::open_default()` (`crates/zet-blocks/src/lib.rs:165–179`) calls it.
  `crates/zet` and `zet-agent` see it transitively.
- **`zet-models` cannot see `zet-blocks`** (no edge, and adding one would be a cycle), so
  `keys.rs:157` and `metered.rs:23` keep their three lines and add the same `$ZET_HOME`
  first-check. Same for `zet-agent::journal::journal_path_default` (`journal.rs:8`).
- **Session identity by env, not by file.** `window/tab.rs` computes `session_id` *before*
  the hook write and replaces `shell.rs:90` with
  `export ZET_SESSION={session_id}\nsource {hook}\nclear\n` written into the PTY. The shell
  exports it; every child — including `zet blocks`, `zet replay`, `zet journal` — inherits it.
  **No `zet-pty` public-interface change** (`Session::spawn` at `crates/zet-pty/src/lib.rs:48`
  takes no env; it does not need to).
- **Consumers default to it.** `blocks()` (`main.rs:366`) filters on `$ZET_SESSION` when
  `--session` is absent; `replay()` (`main.rs:459`) uses it when no session arg is given.
  Only when the variable is set and non-empty — so `cli.rs:189`
  (`zet replay` with no args → exit 2) is preserved.
- **Enforcement.** One test (`crates/zet/tests/state.rs`): with `ZET_HOME=<tmp>`, `zet inline`,
  `zet run --every`, and a recorded block must all land under `<tmp>` and nowhere else.
  That test is the "one path instead of two" guarantee; it catches drift in all four crates.

Spec §8:157's `~/.zet/sessions/<id>/` layout (per-session `blocks.sqlite`, `journal.sqlite`,
`transcript.md`) is **S6, deferred** — it needs a store migration and is not a shipping blocker.

### 8. Migration order

Each step is independently verifiable by the four CI gates, and the tree is shippable after S2.

**S1 — freeze the argv contract (smallest first step, no behavior change).**
New `crates/zet/src/lib.rs` (`pub mod mode;`), new `crates/zet/src/mode.rs`, `[lib]` block in
`crates/zet/Cargo.toml`. `main.rs` is **not touched**; bare `zet` still prints usage and exits 2.
`mode::plan` ships complete and correct with unit tests for all seven rules plus
`__run_worker`, every `CLI_SUBCOMMANDS` entry, bare+display, bare+no-display, quoted phrase,
multi-word, and `bogus`.
*Verify:* all four gates green; `cargo test -p zet` reports 4+11 unit tests.

**S2 — the move (first shippable step).**
`git mv crates/zet-gpu/examples/shell.rs crates/zet/src/window/mod.rs`; rewrite `main()` into
`pub fn run(goal)`; add `pub mod window;`; wire `main.rs` per §5; apply both `Cargo.toml`
diffs; update `docs/SHORTCUTS.md:3–4`.
*Verify:* four gates green **and** `cargo build --release -p zet && ./target/release/zet`
opens the window; `cargo tree -p zet-gpu --edges normal,dev | grep zet-agent` prints nothing;
`cargo tree -p zet-gpu --edges normal | grep -c .` shows only wgpu/winit/cosmic-text/bytemuck/zet-term/zet-pty.

**S3 — session identity.** §7's `ZET_SESSION` injection + the two CLI defaults + the
`state.rs` half of the test. No `zet-pty` change.
*Verify:* four gates; new `cli.rs` case (`HOME=<tmp>`, `ZET_SESSION=s9`, seeded block) →
`zet replay` prints it; `zet replay` with the var unset still exits 2.

**S4 — one root.** `zet_blocks::zet_home()` + `$ZET_HOME` in the three crates that cannot see
it + `main.rs:650/746/955` call it. Finish `state.rs`.
*Verify:* four gates; `ZET_HOME=/tmp/z zet inline off` creates `/tmp/z/inline_spec` and nothing under `$HOME/.zet`.

**S5 — split `window/mod.rs`** into the six files of §3, one file per commit. Pure refactor.
*Verify:* four gates after each commit; `wc -l crates/zet/src/window/*.rs` all < 500.

**S6 (deferred)** — spec §8:157 per-session directory layout; **S7 (deferred)** — move
`__run_worker` (`main.rs:1026`) into `zet-daemon` (today a 1-line stub, `crates/zet-daemon/src/lib.rs:1`)
so background runs stop exec'ing the GUI-linked binary.

## Risks / rejected alternatives

**Option 1 (`zet-gpu::shell`) — rejected, and not on taste.** Two independent failures:
1. *It does not build.* `crates/zet` depending on `zet-gpu` cannot see `zet-gpu`'s
   `dev-dependencies`; `zet_gpu::shell::run()` would need all six crates as **real** deps of
   zet-gpu. The `[dev-dependencies]` placement is not an accident to be preserved — it is the
   thing that makes the current layout unshippable, and Option 1 resolves it the wrong way.
2. *After that promotion, the renderer owns the model stack.* `cargo tree -p zet-gpu
   --edges normal,dev` already shows `zet-agent → zet-models → {keyring, ureq}` and
   `→ zet-context → zet-blocks → rusqlite (bundled C SQLite)` under `zet-gpu`. Shipping that
   makes a renderer crate link a secrets keychain and a C SQLite build, and kills spec §14:1's
   "keep `zet-gpu` backend trait" plan — zet-gpu stops being swappable. Hard reject.

**Option 2 (new `crates/zet-app`) — rejected as ceremony.** It buys nothing Option 3 does not:
`zet-gpu` is pure either way. It costs a workspace member, `Cargo.lock` churn, and a crate
whose only consumer is one binary — while spec §8:139–153 and Appendix B:238 define the crate
list as closed, and §8:142 + Appendix A:223 already designate the `zet` binary as the window
entry point. "Clean separation" is nominal here: `zet-app` would need the identical fan-out,
and `zet` would still link the whole stack.

**Rejected: `include!`/`#[path]` the example from `crates/zet`.** Cross-crate inclusion of
another crate's example is invisible to the module system, breaks `clippy`/`fmt` target
attribution, and still does not grant `crates/zet` access to `zet-gpu`'s dev-deps.

**Rejected: a `[[bin]]` inside `zet-gpu`.** Same dev-dep invisibility for outside consumers,
and it makes the *renderer* crate ship the application binary. Same smell as Option 1.

**Rejected: ship `zet` CLI-only plus a separate `zet-shell` binary.** `docs/SHORTCUTS.md:1–4`
and Appendix A:223 both say bare `zet` opens the window; two binaries split `~/.zet` ownership
and double the launcher story.

**Accepted tradeoffs (measured, stated, not hidden):**
- *Binary size.* `target/debug/zet` is 122 MB today; `target/debug/examples/shell` (the window,
  with the agent stack) is 388 MB. Merging puts the window's stack in one binary. Debug-only
  inflation; release is much smaller. Cost is real but it is the same code that already ships.
- *CLI exec cost.* `zet blocks` runs in **4 ms** today. Linking wgpu/winit/cosmic-text adds
  relocations and page faults at exec. **None of the §7 gates cover CLI exec time** (they cover
  window cold start, input latency, idle RSS, cat-100k, suggest, agent TTFT), so this is a UX
  nicety, not a gate — measure it in S2 and only revisit if it exceeds ~25 ms.
- *Cold start (§7: <300 ms exec → first prompt frame) is already at risk, unrelated to this
  change.* Measured on this machine: `perf: mark main 0ms`, `shell_spawn 40ms`,
  `renderer_new 71ms`, `wgpu_adapter 285ms`, `wgpu_device 367ms`. wgpu adapter+device creation
  alone is 285–367 ms. This refactor does not change that code path, but the gate cannot be met
  without attacking adapter/device init separately. Flagged, out of scope for A1.
- *`zet __run_worker`* (`main.rs:1026`, spawned by `run --background`) will exec a
  GUI-linked binary. Mitigated by S7 (daemon owns background runs).

**On `zet-agent → zet-models`: acceptable in `crates/zet`, never in `zet-gpu`.** `zet-agent`
is a logic crate, not a render crate, and `crates/zet-agent/Cargo.toml:7–9` already has
`zet-models`, `zet-policy`, `zet-context` — no cycle, no upward edge from a leaf. The rule to
enforce is the one in §1: a *render* crate must never acquire that transitive closure. Option 1
is exactly that violation; Option 3 is not.

## Acceptance checks

Run from `/home/bandit/Projects/zet` unless noted.

**Baseline (must hold before and after every step).**
```bash
cargo fmt --all -- --check                        # exit 0
cargo check --workspace --all-targets             # exit 0
cargo clippy --workspace --all-targets -- -D warnings   # exit 0
cargo test --workspace                            # exit 0
```
Verified green on master (`6b28e71`) before this design: fmt 0, check 0, clippy 0, test 0
(`zet`: 4 unit + 37 `cli.rs`).

**S2 — the edge is gone and the binary ships.**
```bash
cargo tree -p zet-gpu --edges normal,dev | grep -E 'zet-agent|zet-models|zet-blocks|keyring|ureq|rusqlite|notify-rust'
# expect: no output
cargo build --release -p zet && ls -l target/release/zet
# expect: exists; ./target/release/zet opens the window
cargo run -p zet -- blocks --limit 1        # expect: exit 0, blocks listed, unchanged
cargo run -p zet -- bogus                   # expect: exit 2, usage on stderr  (cli.rs:20)
cargo run -p zet -- auth --definitely-bogus # expect: exit 2                    (cli.rs:274)
ZET_NO_WINDOW=1 cargo run -p zet            # expect: exit 2, usage (headless-safe path)
```

**S3 — one session identity.**
```bash
ZET_SESSION=<id-from-the-window> cargo run -p zet -- blocks   # lists only that session
ZET_SESSION=<id> cargo run -p zet -- replay                   # prints that transcript
cargo run -p zet -- replay                                    # expect: exit 2 (cli.rs:189)
```
Inside the running window: `echo $ZET_SESSION` prints the id that `zet blocks` filters on.

**S4 — one root.**
```bash
ZET_HOME=/tmp/zethome cargo run -p zet -- inline off
find /tmp/zethome -type f        # expect: inline_spec (+ blocks.sqlite/journal.jsonl when used)
find "$HOME/.zet" -newermt '-1 minute'   # expect: nothing new
```

**Perf guard (S2, not a gate — a regression tripwire).**
```bash
hyperfine './target/release/zet blocks --limit 1'   # before vs after; must stay < ~25 ms
ZET_PERF=1 ./target/release/zet 2>&1 | grep 'mark shell_spawn'
# expect: same order as today's 40 ms; must not regress > 10 ms
```
