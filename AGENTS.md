> **CURRENT PRODUCT AUTHORITY (2026-10-22):** ZET — **The Outcome-Aware Terminal** (provisional descriptor; ZET is the brand). Read [docs/ZET-PRODUCT-CHARTER.md](docs/ZET-PRODUCT-CHARTER.md) and [docs/ROADMAP.md](docs/ROADMAP.md) **before proposing features or UI changes**. Earlier agent-centric positioning and original roadmap in `docs/ZET-DESIGN.md` are historical where inconsistent. Build an evidence-connected native terminal and Mission Control, **not** another Hermes/OpenClaw or a browser app. Preserve every security, performance, terminal-compatibility, and owner-approval gate below.

# Zet — AI-native terminal emulator

Repo: github.com/kryptek2000/zet (Rust + wgpu). Full spec: `docs/ZET-DESIGN.md`. Architecture diagram: `zet-architecture.html`.

## Workspace layout

```
crates/
  zet          binary + CLI (zet, zet run, zet replay, zet undo, zet audit)
  zet-term     VT parser, grid, scrollback, shell integration (OSC 133/633)
  zet-pty      portable PTY (fork/exec, Win ConPTY), SSH transport
  zet-gpu      wgpu renderer, glyph atlas (cosmic-text/parley), damage tracking
  zet-blocks   block store (SQLite + Arrow IPC for scrollback)
  zet-context  repo map, shell history, env, git watcher
  zet-agent    planner/executor/verifier, lane scheduler, checkpoints
  zet-policy   tiers, approvals, sandbox profiles, audit log
  zet-models   LLM gateway trait: LocalLlama, OpenAI-compat, Anthropic, BYOK
  zet-suggest  inline autocomplete (local 1B model, <50ms)
  zet-mcp      MCP client/host, tool registry, third-party CLI panes
  zet-daemon   background/delegated runs, notifications, zet status IPC
```

## Non-negotiable principles (spec §2)

1. Terminal first, chat second — AI renders as blocks, never hijacks the PTY.
2. Speed is a feature — the perf budgets below are release gates.
3. Agent is accountable — every action = visible block + diff + audit entry + undo.
4. Local-first, BYOM — no account required to open a shell.
5. Ask rarely, at the right time — tiered approvals (T0–T3), not Y/N spam.
6. Keyboard everything.
7. Standards over lock-in (xterm, OSC, MCP, Markdown, TOML).
8. No silent writes — no mutation without a recorded checkpoint.

## Perf gates (spec §7) — release gates, not aspirations

| Metric | Gate |
|---|---|
| Cold startup (exec → first prompt frame) | < 300 ms steady |
| Input latency (key → pixel, warm) | < 3 ms steady-state |
| Idle RSS | < 100 MB |
| `cat 100k lines` | < 0.9 s |
| Inline suggest | < 50 ms |
| Agent first-step TTFT | < 4 s cloud |

Threading: UI/GPU thread ↔ PTY reader ↔ agent tokio runtime ↔ model streaming, all cross via lock-free rings. **PTY output must never block on model I/O.** Never put blocking calls on the UI thread.

## Safety tiers (spec §6.4)

| Tier | Examples | Default |
|---|---|---|
| T0 read | cat, ls, git status, kubectl get | auto |
| T1 low-write | file edit in repo, npm install, git commit | auto in repo, ask outside |
| T2 risky | git push, docker run, db migrate, `rm -rf`, curl\|sh | always ask + show exact cmd + diff |
| T3 critical | prod kubectl, secrets access, sudo, mass delete | ask + type-to-confirm + sandbox only |

Checkpoints: pre-write snapshot before every mutation; all agent writes journaled to `~/.zet/journal/<session>.sqlite`. Secrets (`~/.aws`, `~/.ssh`, env) are never sent to cloud models unless explicitly attached.

## Build, test, verify

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI (`.github/workflows/ci.yml`) runs exactly these four on every push and PR, plus installs `zsh` for the zet-pty zsh integration test. **All four must pass before a change is considered done.** The zsh test is non-interactive and needs an empty `.zshrc` to skip the newuser wizard — don't "fix" that by changing the test; the CI passes today.

## Working rules for agents

- Match the surrounding style: existing error handling, naming, module layout, doc-comment density.
- No `todo!()`, no stubbed functions left behind, no placeholders.
- Keep diffs small and reviewable. One concern per change.
- Never silently change another crate's public interface — flag it instead.
- Cite real paths and real command output. Never claim a check passed that wasn't run.
- Ask the Architect for the design when the crate boundaries or a public interface are unclear.
- The Auditor reviews; the Implementor edits. Don't blur that.
