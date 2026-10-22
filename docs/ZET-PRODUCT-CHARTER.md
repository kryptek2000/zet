# ZET — Product Charter: The Outcome-Aware Terminal

**Approved direction:** 2026-10-22 · **Status:** Flagship vision approved; implementation milestones proposed  
**Product name:** ZET · **Descriptor:** The Outcome-Aware Terminal (provisional, not cleared as a trademark)  
**Final design, merge and release authority:** Project Owner

## Product promise

ZET is a **native, fast Linux-first terminal** that makes development work **connected, inspectable, controllable and verifiable**. It turns isolated shell activity into an optional, evidence-linked workstream without replacing the real PTY.

A developer can open ZET as a conventional terminal. When a task becomes complicated, Mission Control reveals the commands, sessions, files, agents (when observable), approvals, checkpoints and test/CI evidence related to that objective.

**Defining test:** Can a developer move from a failed test to a verified fix with less context switching and greater trust than using plain terminal windows, without being forced to adopt a new AI platform?

## Naming rule

Only **ZET** is the product name. **“The Outcome-Aware Terminal”** is our proposed distinctive public-facing descriptor, pending legal/trademark review. Preliminary exact-phrase web checks on 2026-10-22 did not reveal a matching terminal product, but search cannot establish exclusivity. Do **not** promote “The Living Terminal” as a product name: similar language is used by others. “Mission Control” is a descriptive in-app surface label, not asserted exclusive.

## Non-negotiables

1. **Native app:** Rust + wgpu, no Electron, Chromium, browser or embedded webview requirement. Linux Wayland and X11 are key targets.
2. **Terminal integrity:** Real shell/PTY interaction, true scrollback, keyboard protocols, editors and multiplexers remain correct. Mission chrome must not substitute a fake terminal transcript.
3. **Useful without AI:** Users can navigate local commands, results and manually linked missions offline and without login. AI is enhancement, not prerequisite.
4. **No competing agent platform:** Existing coding CLI agents operate in ZET's real terminal. Optional deep integrations require explicit contracts; a user can keep their preferred agent.
5. **Honest provenance:** Separate observed events, explicit user linkages, inferred associations, requested work, executed operations, and independently verified outcomes. Unknown stays unknown.
6. **Human control:** Respect actual zet-policy T0–T3 classification. Approval gates, diff scope, checkpoint and undo state must be real. No silent writes or automatically claimed success.
7. **Performance:** preserve the existing release gates (startup, input, RSS and output throughput), demand-driven rendering, no PTY stall on model or index work. No fake FPS or latency figures.
8. **Discoverability:** Modes/layouts plainly accessible in native chrome, via keyboard and mouse. Switching views must not restart a terminal, lose scrollback or hide pending approvals.
9. **Release authority:** Implementation is peer-reviewed and CI-green; owner decides merges/releases.

## Approved flagship presentation

**Left / Mission rail (collapsible):** Mission objectives, active PTY sessions and evidence-backed status. A mission may include several terminals and manual actions; it is not synonymous with an agent.

**Center / Living workstream:** Real terminal output, with optional anchored command blocks, failure summaries, diff references, actual approvals, checkpoints and verification receipts. Terminal input is never blocked behind the overlay.

**Right / Inspector (collapsible):** Current mission's timeline, facts, files, agent observations, verification status and pending decisions. No permanent mandatory AI chat.

**Status / Lightweight:** actual workspace, model/provider if connected, selected branch and meaningful system telemetry—truthful and optionally hidden.

**Presentation layouts (proposed):** Terminal Focus, Workspace, Mission Control; these are layouts on **one live native app**, not different runtimes. Exact labels and shortcuts require a conflict audit against existing Ctrl+G and documented bindings before implementation.

## What makes the experience distinct

**Outcome chain:** command → observed result → linked artifacts → subsequent action → verification evidence. Each edge identifies a grounded source or explicit user choice. A command completing with exit 0 does not certify the whole mission; a green-looking UI is not a substitute for actual checks.

**Interaction test:** a user can inspect the raw failed command, select an error, see related *observed* facts, initiate investigation with an existing CLI, review its real diff, withhold approval, and later view the exact successful test/CI evidence. Every operation remains recoverable where the underlying facilities support it.

## Visual direction

Approved mockup: futuristic, refined, dark graphite/deep-space surfaces, restrained electric blue/cyan/violet, polished native controls, clear type hierarchy, integrated mission/context panels. Avoid an always-visible cluttered dashboard, excessive animations, decorative fake indicators, or requiring panels to use the terminal.

The mockup is *visual reference only*: no synthetic sessions, tests, agent telemetry, benchmark figures, approval success or verification claims may ship as real state.

## Roles and authority

- **Product owner:** final creative direction and release/merge approval.
- **GPT-6:** system architect, product/UX spec and reviewer.
- **MiniMax:** proposed UI implementer after a small native vertical-slice proof.
- **Beam:** independent UI auditor; implementation roles may be reversed for other tasks.
- **Audit principle:** author never signs off on their own change.

## Scope boundaries

Not a Hermes/OpenClaw clone, hosted orchestration service, agent marketplace, new shell language, IDE replacement or automation factory. Existing repository agent/runtime work may remain available but **no new feature has priority merely because it is AI-powered**. Core terminal compatibility and genuine work visibility win.

## Acceptance signals

- No feature renders a fabricated state.
- Terminal Focus usable and fast independent of Mission Control.
- A mission's command→failure→artifact→verification trail is grounded in source evidence.
- Pending approvals stay visible and cannot be bypassed by presentation mode switches.
- Full Rust CI green, targeted tests, independent audit, benchmarks compared with established baseline.
- Usability evaluation demonstrates lower friction than a standard multi-tab terminal workflow.

See [ROADMAP.md](ROADMAP.md) for sequencing and proof gates.
