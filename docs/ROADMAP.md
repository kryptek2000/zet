# ZET — Delivery Roadmap

**Updated:** 2026-10-22 · **Authority:** [Product Charter](ZET-PRODUCT-CHARTER.md)  
**Product positioning:** ZET — The Outcome-Aware Terminal (provisional descriptor).  
**No dates promised:** Release when correctness, usability, and measured performance meet gates.

## P0 — Restore and protect the foundation

- Close renderer clipping/sidebearing CI repair only after independent approval; PR #13 is not considered merged merely because PR CI is green.
- Confirm **master** green after the owner-approved merge.
- Inventory actual implemented native features versus planned designs; no greenwashing README.
- Capture reproducible baseline: startup, input, RSS, output, fonts/Unicode, Neovim/tmux/SSH, Wayland/X11 where testable.

**Exit:** green protected baseline, documented known limitations, repeatable perf and compatibility smoke tests.

## P1 — UX and technical contract

- Reconcile old `docs/ZET-DESIGN.md` §5 and §13, historical A4 UI spec, the current renderer/window event contract and new charter. Mark superseded directions.
- Define `Mission`, `WorkEvent`, `ArtifactRef`, `EvidenceLink`, `ApprovalState`, and `VerificationReceipt` with provenance and uncertainty.
- Specify terminal/workspace/mission layouts and discoverable mode switcher; audit keybinding conflicts.
- Define precise semantics of “observed,” “inferred,” “verified,” and the user's approval.
- Break native UX into component contracts and screen-state drawings based on approved mockup.

**Exit:** architect-approved native UI blueprint, state schema and testable acceptance criteria; no implementation based on HTML.

## P2 — Native vertical slice (MiniMax implementer; Beam auditor)

- Render collapsible mission rail, honest central PTY and optional inspector **inside existing Rust + wgpu app**; do not add Electron/webviews.
- Use **real existing PTY/session data** for mission and inspector example. No hardcoded simulated work progress.
- Demonstrate layout switching **without losing input, sessions or scrollback**; accessible controls and a genuine focus mode.
- Bench frame pacing, startup, idle CPU/RSS, resize, thousands of lines and interaction latency against P0.

**Exit:** playable Linux native prototype with focused tests, screenshot comparison to approved design, a green PR and independent audit.

## P3 — Evidence-linked commands and mission timeline

- Associate real shell commands with captured output/exit/duration, cwd/git identity and session IDs via existing block infrastructure where available.
- Create local mission grouping; allow manual relationships and explicit provenance when automatic linkage is uncertain.
- Navigate command → failure → raw output → real file/diff refs; ensure links update appropriately or mark them stale.
- Build time-ordered timeline with immutable event identity and honest replay limitations.

**Exit:** demonstrated real failed-build-to-investigation navigation, no false attributions, controlled data storage.

## P4 — Agent awareness and human decisions

- Keep arbitrary agent CLIs usable without hooks. Observe only trustworthy signals; unknown agent state remains unknown.
- Optional structured integrations only with user consent and security review.
- Render T0–T3 classifications accurately; actionable approval surfaces reflect actual enforcement, not simulated clicks.
- Link proposed edits, permission decisions, checked-in diffs, checkpoints and rollback outcomes only when verified.

**Exit:** live agent workflow demonstrates inspectable evidence and approval enforcement without hijacking the PTY.

## P5 — Verification and usefulness

- Link local test output and GitHub CI checks to **exact commit identity**. Distinguish local test success, remote CI green, and mission-completion judgment.
- Demonstrate failed command → investigated change → verified result as a complete provenance-backed trace.
- User studies: compare conventional terminal vs ZET on CI triage and code-review tasks; record completion time, errors, context switches and trust.
- Performance and accessibility: wide output, terminal apps, fonts/CJK, input latency, memory, scaling, navigation, reduced motion.

**Exit:** measurable workflow improvement, no critical compatibility failures, independent audit and owner sign-off.

## P6 — Polish, portability and release

- Native themes, keyboard customization, persistence, Wayland/X11 behavior, packaging, docs, onboarding and truthful telemetry.
- Grow support for standards (graphics protocols, hyperlinks, terminal width/fallback) according to demonstrated demand and compatibility.
- Security/red-team, release benchmarking, regression gates, documentation and packaging.
- Mac/Windows after Linux stability and explicit architecture review.

**Exit:** release candidate accepted by owner after stable CI, cross-platform/compatibility evidence and documented limitations.

## Deliberately deferred / not our differentiation

New autonomous orchestration platform, agent marketplace, cloud automation factory, subscriptions, multi-agent control plane, obligatory AI models, dashboard-only product. Earlier `ZET-DESIGN.md` M1/1.0/2.0 projections are **historical aspirations**, not current controlling roadmap.

## Implementation governance

PR per bounded phase; no surprise direct pushes to master; author implements, independent auditor checks; architect reviews; owner merges. Every feature must have a truth source, degradation state, keyboard behavior, security test, performance budget and a meaningful user job.
