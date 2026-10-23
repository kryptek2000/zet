> **CURRENT PRODUCT AUTHORITY (2026-10-22):** ZET — **The Outcome-Aware Terminal** (provisional descriptor; ZET is the brand). Read [docs/ZET-PRODUCT-CHARTER.md](docs/ZET-PRODUCT-CHARTER.md) and [docs/ROADMAP.md](docs/ROADMAP.md) **before proposing features or UI changes**. Earlier agent-centric positioning and original roadmap in `docs/ZET-DESIGN.md` are historical where inconsistent. Build an evidence-connected native terminal and Mission Control, **not** another Hermes/OpenClaw or a browser app. Preserve every security, performance, terminal-compatibility, and owner-approval gate below.

# ZET — Native terminal engineering guide

Repo: github.com/kryptek2000/zet (Rust + wgpu). Full spec: `docs/ZET-DESIGN.md`. Architecture diagram: `zet-architecture.html`.

## Workspace layout — implemented versus planned

**Important:** A crate directory or an old architecture design does **not** mean the feature is implemented. The inventory below reflects the code review in `docs/design/A5-mission-control-architecture.md` at the PR #13 head (`166c204`); verify against current code before claiming a feature is shipped.

| Crate | Present implementation / limitation |
|---|---|
| `zet` | Native executable, local shell/window composition, tabs, existing agent interaction; earlier CLI sketches contain unimplemented commands |
| `zet-term` | VT grid, scrollback, alt-screen, bracketed paste, shell integration; resize **does not reflow** existing wrapped rows |
| `zet-pty` | Local fork/exec PTY and resize; SSH transport and Windows ConPTY are **not implemented in this baseline** |
| `zet-gpu` | wgpu renderer, cosmic-text glyph atlas, tab bar, ghost/search/selection; pure `Damage` structures exist but **damage-based rendering is not wired** |
| `zet-blocks` | SQLite command block store/recorder and secret scrubbing; Arrow IPC is **not implemented** |
| `zet-context` | Git state, context gathering and scrubbing; repo map, external history reader, env reader and file watcher are **not implemented** |
| `zet-agent` | Single-lane planner/executor/verifier, journal/checkpoint support; parallel lane scheduler is **not implemented** |
| `zet-policy` | T0–T3 tier classification and approval decisions; OS sandbox profiles and separate audit-log subsystem are **not implemented** |
| `zet-models` | Model gateway / BYOK plumbing; provider readiness depends on local configuration |
| `zet-suggest` | Inline suggestion/history support; do not advertise universal sub-50ms model inference |
| `zet-mcp` | MCP registry and stdio tool/server integration; third-party CLI panes are **not implemented** |
| `zet-daemon` | Stub only; background/delegated execution and notification IPC are **not implemented** |

**Product implementation order:** [Charter](docs/ZET-PRODUCT-CHARTER.md) → [Roadmap](docs/ROADMAP.md) → [A5 native Mission Control proposal / PR #16](https://github.com/kryptek2000/zet/pull/16). A5 is subject to independent audit and owner merge approval; it is **not** implemented UI. Historical diagrams and `ZET-DESIGN.md` contain proposed future capabilities.

## Non-negotiable principles (spec §2)

1. Real terminal integrity first — Mission Control is optional chrome around the canonical PTY; AI must never hijack it.
2. Speed is a feature — the perf budgets below are release gates.
3. Agent accountability is a requirement — actions must be auditable and show available evidence; do not imply an unimplemented audit or undo capability.
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

## Safety tiers — intended policy, not a sandbox implementation (spec §6.4)

| Tier | Examples | Default |
|---|---|---|
| T0 read | cat, ls, git status, kubectl get | auto |
| T1 low-write | file edit in repo, npm install, git commit | auto in repo, ask outside |
| T2 risky | git push, docker run, db migrate, `rm -rf`, curl\|sh | always ask + show exact cmd + diff |
| T3 critical | prod kubectl, secrets access, sudo, mass delete | ask + type-to-confirm + sandbox only |

**Enforcement boundary:** The existing agent path supports checkpoints/journal entries, but a local shell command or third-party CLI is not automatically journaled or safely undoable. OS sandboxing, comprehensive audit logs and complete secret-control guarantees require additional implementation and verification. Never advertise these as present just because the policy design specifies them. Treat sensitive local data as untrusted for model submission.

## Build, test, verify

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI (`.github/workflows/ci.yml`) runs exactly these four on every push and PR, plus installs `zsh` for the zet-pty zsh integration test. **All four must pass before a change is considered done.** The zsh test is non-interactive and needs an empty `.zshrc` to skip the newuser wizard — do not "fix" that by changing the test. **Always inspect the latest run; at this documentation update, master still had an outstanding text-rendering test failure pending PR #13.**

## Working rules for agents

- Match the surrounding style: existing error handling, naming, module layout, doc-comment density.
- No `todo!()`, no stubbed functions left behind, no placeholders.
- Keep diffs small and reviewable. One concern per change.
- Never silently change another crate's public interface — flag it instead.
- Cite real paths and real command output. Never claim a check passed that wasn't run.
- Ask the Architect for the design when the crate boundaries or a public interface are unclear.
- The Auditor reviews; the Implementor edits. Don't blur that.
