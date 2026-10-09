# ZET
A modern, ultra-fast AI-native terminal emulator — Ghostty-class speed, Warp-beating agentic runtime.

> Terminal that does the work, not just the commands.

## Docs
- **Full spec:** [`docs/ZET-DESIGN.md`](docs/ZET-DESIGN.md) — product, agentic core (plan→gate→exec→verify), safety tiers, perf gates, roadmap
- **Architecture:** [`zet-architecture.html`](zet-architecture.html) — open in a browser (offline, no deps)
- **Workspace:** `Cargo.toml` + `crates/` — `zet-term, zet-pty, zet-gpu, zet-blocks, zet-context, zet-agent, zet-policy, zet-models, zet-mcp, zet-daemon`

## Edge (vs Warp / Ghostty / Amazon Q)
- Real-terminal agent: plans, runs, verifies, rolls back — in your shell, over SSH, headless via `zet run --bg`
- Tiered approvals (T0 auto → T3 type-to-confirm + sandbox) + checkpoint every mutation + one-key undo
- Local-first: offline inline <50ms, BYOM/cloud gateway, no login to launch
- Perf gates: <100ms cold, <3ms input, 120fps, <80MB idle — CI-benched vs Ghostty/Warp

## Quickstart (target CLI)
```bash
zet                                   # open window
zet "fix failing e2e in ./api"         # open with agent goal
zet run --bg recipe:triage-ci --arg service=api
zet undo                              # revert last agent checkpoint
zet replay <session>                  # replay timeline
```

Requires Rust toolchain (not yet installed in this env) — `cargo build` once `rustup` is available.
