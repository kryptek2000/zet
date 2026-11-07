# ZET — Delivery Roadmap

**Updated:** 2026-11-07 · **Authority:** [Product Charter](ZET-PRODUCT-CHARTER.md)\
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

### Completed in merged master

- P2-B1 native Mission Rail, tab and chrome polish is integrated into master (PR #35, commit `6b76b3de92e9c5d65187bb944e63aa2642065ce3`).
- P2-B2 Contextual Inspector foundation is integrated into master (PR #43, commit `01a431d120207bca3c6f667e469f61a4e1464e0b`).
- P2-B2C integration repair and session-identity repair are merged.
- P37-A/P37-D additive startup instrumentation is available on master.
- Independent audit, owner visual approval and exact-head CI passed for the integrated candidate.
- Release performance gates remain open: startup < 300 ms, input < 3 ms, idle RSS < 100 MB, `cat 100k` < 0.9 s.

### P2-B — Flagship native UI fidelity

Planning anchor: [Issue #29](https://github.com/kryptek2000/zet/issues/29) and the
[Flagship UI Reference](design/ZET-FLAGSHIP-UI-REFERENCE.md). Preserve the approved
native composition—**Mission Rail | Living Terminal Workstream | Contextual
Inspector**—and deliver it through small, independently audited slices. Do not
substitute a competing layout or a generic dashboard.

- **ZET Signature ASCII Identity.** Add a distinctive, large, monospace
  ASCII-style **ZET** wordmark to the native welcome/startup and empty-workspace
  experience. It must clearly read ZET, remain legible without color, work with
  selectable themes and narrow windows, and be optional or dismissible. It must
  never inject fake PTY content, overwrite shell output, obstruct
  input/scrollback, or fabricate system data. Exact artwork and typography
  require separate owner approval before implementation. See the
  [P2-B ASCII identity note](design/P2B-ZET-ASCII-IDENTITY.md).

Implementation of P2-B requires a separate owner-authorized slice; this
subsection is a planning and visual-acceptance requirement, not authorization
to start UI work.

## P3 delivery status — merged and installed

- **P3-B1 — Mission Intelligence Foundation:** merged into master through PR #62; local mission storage, user-linked command evidence, and CLI operations are implemented.
- **P3-B2 — Native Mission Intelligence:** merged through [PR #63](https://github.com/kryptek2000/zet/pull/63) at `5a01670704342926a798fa375ea5beecd70b32cf`; canonical owner installation confirmed 2026-11-07. Native create/select/link/inspect/unlink works and was owner-smoke-tested with real command associations.
- These slices establish an **evidence-linked local foundation**, **not** a complete AI-agent control plane, persistent live-process manager, automatic mission verification system, or proof of external user demand.
- Remaining UI polish includes Inspector heading spacing, evidence readability, clipped status text, and welcome artwork/modal layering; schedule by demonstrated impact rather than allowing polish to obscure terminal reliability.
- Some full release performance and compatibility gates remain open. A green feature PR does not certify all product-level gates.

## Next milestone — P3-B3: Developer usefulness discovery (research and thin experiments)

**Decision:** Validate why someone would voluntarily choose ZET over their current terminal. Do not authorize a large new feature merely because it is AI-powered. This is a product-discovery milestone, not blanket authorization for implementation or release.

**Primary user:** Developers regularly using shell commands and existing AI coding CLIs such as Claude Code, Codex CLI, Gemini CLI, or OpenCode. Their CLIs remain independent programs inside the genuine PTY. ZET must continue to be useful without models, accounts, or a network connection.

### Two competing workflow hypotheses

1. **AI CLI supervision:** Can a developer operate a real coding CLI, recognize genuinely observed waiting/working/error states, find its relevant session and output, and resume with fewer context switches? Unknown state must be labeled unknown. No agent status inferred merely from terminal appearance, no forced proprietary agent integration.
2. **Command evidence and work recovery:** After a failed build or interrupted session, can a developer find the recorded command, inspect raw output and grounded metadata, recover work context, and continue faster and more accurately? Manual mission linking is a baseline to measure, not proof of effortless capture or end-to-end verified outcomes.

### Discovery protocol

- **Baseline:** Use the same realistic task with the participant's preferred conventional terminal/CLI workflow, then with ZET. Counterbalance task order and capture prior familiarity; do not claim comparative superiority from screenshots or self-reported enthusiasm.
- **Internal dogfooding first:** Run repeatable AI-CLI and failed-command recovery tasks using isolated profiles and non-sensitive repositories. Keep full-screen TUI support, keyboard/input semantics, paste, resize, scrolling, focus, and PTY throughput under test.
- **Outside developers:** Aim for 5–10 participants spanning frequent CLI users and AI-agent CLI users. Observe real tasks; record frustrations, workarounds, and what they actually use. This is exploratory, not statistical proof of market demand.
- **Metrics:** Completion rate, task time, context switches, wrong attributions, needed manual steps, terminal compatibility defects, and whether participants voluntarily return after several days. Capture qualitative trust/discoverability observations.
- **Provisional decision signals:** Seek roughly 20% less task time without lower completion rates; zero critical terminal regressions; at least 3 of the first 5 participants electing to use ZET again. Treat these as hypotheses to evaluate, not release promises or claims of demonstrated performance.
- **Competitive benchmark:** Evaluate actual workflows against Warp, Wave, Herdr, and plain terminal + preferred AI CLI where relevant. Avoid cloning product categories based only on feature checklists.
- **Evidence and privacy:** Opt in to any research logging; scrub secrets, avoid uploading private transcripts without explicit consent, and distinguish reported perceptions from directly observed results.

### Gate before larger P3-B3 engineering

Produce a short findings report with reproductions and performance/compatibility evidence, then choose **one** bounded next slice by observed user value. Acceptable outcomes are: pursue AI-CLI ergonomics, pursue evidence/recovery ergonomics, redesign an unhelpful interaction, or stop and reassess. The architect reviews, the independent auditor checks relevant implementation claims, and the owner authorizes any build/merge/install.

**Not automatically authorized:** new built-in agent orchestration, autonomous multi-agent scheduling, account/service dependencies, rich media rendering, native split panes, custom model inference, plugins, or a general UI overhaul. These remain candidates only if discovery justifies them.

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
