# ZET — The Outcome-Aware Terminal

**A native Linux-first terminal that connects commands, development work, and verified results.**

> Terminal that does the work, not just the commands.

**ZET** is the product name. **“The Outcome-Aware Terminal”** is the proposed descriptive positioning, subject to further naming/trademark clearance—not a claim of exclusive rights.

ZET is a genuine **Rust + wgpu desktop terminal** with an ambitious new layer: **Mission Control**. The goal is to connect real commands, errors, files, changes, human decisions, agent activity, checkpoints, and verification into an inspectable development workstream.

## Our defining experience

A failed test is more than red text: ZET should let you follow its original command and output, related files, the subsequent investigation, the exact approved changes, and the evidence that a fix actually worked. Any association must have known provenance; ZET must never invent relationships or falsely report verification.

- **The real terminal remains central.** PTY, shell, SSH, editors, and conventional terminal programs work normally, including without AI.
- **Mission Control makes the work navigable.** Missions group objectives, commands, real sessions, decisions and outcomes—not just tabs or agent processes.
- **An optional agent-aware layer.** Existing coding CLIs can run unchanged. ZET must not become another Hermes/OpenClaw or require its own autonomous agent to be useful.
- **Evidence over claims.** Proposed, executed, and verified are distinct statuses; approvals, diffs, checkpoints, and undo reflect the actual underlying systems.
- **A beautiful native interface.** Futuristic dark design, collapsible mission rail and contextual inspector, central living workstream; one-click/key focus layout.
- **Terminal-grade performance and trust.** GPU acceleration, demand-driven frames, strong compatibility, local-first operation, keyboard accessibility and policy gates.

## What ZET is not

No Electron, Chromium shell, webview UI, or browser required for operation. No forced AI login. No generic agent orchestration dashboard. No synthetic metrics represented as live telemetry.

## Current status

**In development.** The native terminal and several infrastructure crates already exist. The flagship Mission Control interface and linked evidence model are **planned**, not shipped; the visual mockup is a **design reference**, not implementation evidence. The Rust test suite and CI must remain green before release.

## Build and verify

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## The definitive plan

- [Product Charter / Approved Direction](docs/ZET-PRODUCT-CHARTER.md) — authority, UX, boundaries and naming
- [Execution Roadmap](docs/ROADMAP.md) — ordered milestones and acceptance criteria
- [Architecture and performance baseline](docs/ZET-DESIGN.md) — original system design, retained for implementation constraints; superseded where positioning conflicts with charter
- [Engineering rules](AGENTS.md) — authoritative security/performance/testing gates
- [UI component design reference](docs/design/A4-ui-component-spec.md) — historical design proposal, to reconcile before implementation
- [Shortcuts](docs/SHORTCUTS.md) — existing behavior (do not break it to implement mockup shortcuts)

**Decision hierarchy:** product vision and scope: Product Charter → Roadmap. Implementation: code, engineering rules, technical specs. Security, performance and compatibility protections cannot be relaxed by marketing copy.
