# ZET — Delivery Roadmap

**Updated:** 2026-10-30 · **Authority:** [Product Charter](ZET-PRODUCT-CHARTER.md)\
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

## Next milestone — P3: Evidence-linked commands and mission timeline

**Current next product milestone.** Begin with an architect-approved bounded contract using existing command recording and provenance. Do not implement P3 in a documentation-only sync.

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

### Approved terminal compatibility certification program

**NVIM-1 — Full Neovim/TUI editor acceptance.** Revisit Neovim on the current ZET flagship after the fixed E1568 background-query defect (Issue #36 / PR #40). Test colors/truecolor, syntax, Normal/Insert modes, cursor and keyboard input, resizing, splits, redraws, Unicode/box drawing, alternate screen, clipboard, clean exit, and Neovim inside tmux. The previous focused E1568 fix is **not** comprehensive Neovim certification.

**COMPAT-1 — Native Terminal Compatibility Certification.** Run reproducible protocol tests and supervised real-app smoke tests covering:
- P0 critical: Bash/Zsh/Fish; Neovim/Vim/Nano/Helix; tmux/GNU Screen; OpenCode/Gemini CLI/Claude Code/Codex CLI where available; ANSI/VT SGR/CSI/OSC, default colors, reverse video, cursor and alternate screen; keyboard modifiers, paste, resize and core terminal input integrity.
- P1 high: htop/btop/lazygit/fzf/less; Unicode/CJK/combining marks/emoji/box drawing; clipboard/OSC 52 and hyperlinks/OSC 8 where supported; SSH/remote tmux; Wayland/X11, display scaling and monitor changes.
- P2 conditional: Sixel, Kitty graphics, other advanced image protocols or platform-specific integrations. Unsupported protocols must be documented, not silently claimed.

**STAB-1 — Native stability and recovery.** Bounded multi-hour use with multiple tabs, editor/agent switching, repeated resize, heavy output, idle memory trends, terminal responsiveness, interrupted SSH/agent processes and process recovery. Safeguard sessions and user data; no uncontrolled stress or destructive operations.

**SEC-1 — Terminal-input and data-integrity security.** Fuzz or regression-test malformed escape sequences, oversized/untrusted terminal data, OSC handling, bounded recording, secret redaction, session isolation, storage failures, and approval/enforcement truthfulness. No claim of comprehensive sandboxing without enforcement evidence.

The certification matrix begins at **NOT RUN** for every unverified individual case; a historical targeted fix or a green unit suite is not certification of a full app. Use exact statuses **PASS / FAIL / BLOCKED / NOT RUN / NOT SUPPORTED**, linked to app/version, ZET SHA, environment, steps, and raw evidence. Distinguish unsupported-but-optional P2 capabilities from required P0 failures. Require independent audit and owner acceptance; do not claim full compatibility until supported by actual evidence.



- Link local test output and GitHub CI checks to **exact commit identity**. Distinguish local test success, remote CI green, and mission-completion judgment.
- Demonstrate failed command → investigated change → verified result as a complete provenance-backed trace.
- User studies: compare conventional terminal vs ZET on CI triage and code-review tasks; record completion time, errors, context switches and trust.
- Performance and accessibility: wide output, terminal apps, fonts/CJK, input latency, memory, scaling, navigation, reduced motion.

**Exit:** measurable workflow improvement, no critical compatibility failures, independent audit and owner sign-off.

## P6 — Polish, portability and release

- Native themes, keyboard customization, persistence, Wayland/X11 behavior, packaging, docs, onboarding and truthful telemetry.
- Grow support for standards (graphics protocols, hyperlinks, terminal width/fallback) according to demonstrated demand and compatibility.
- Security/red-team, release benchmarking, regression gates, documentation and packaging.
- **P6 pre-release checkpoints:** complete and independently audit applicable NVIM-1, COMPAT-1, STAB-1 and SEC-1 P0 gates; explicitly classify P1 limitations and P2 conditional protocols, publish a compatibility matrix and obtain owner acceptance. Do not waive required gates silently.
- Mac/Windows after Linux stability and explicit architecture review.

**Exit:** release candidate accepted by owner after stable CI, cross-platform/compatibility evidence and documented limitations.

## Deliberately deferred / not our differentiation

New autonomous orchestration platform, agent marketplace, cloud automation factory, subscriptions, multi-agent control plane, obligatory AI models, dashboard-only product. Earlier `ZET-DESIGN.md` M1/1.0/2.0 projections are **historical aspirations**, not current controlling roadmap.

## Implementation governance

PR per bounded phase; no surprise direct pushes to master; author implements, independent auditor checks; architect reviews; owner merges. Every feature must have a truth source, degradation state, keyboard behavior, security test, performance budget and a meaningful user job.
