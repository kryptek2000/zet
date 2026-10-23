# ZET — Flagship UI Reference and Iteration Rules

**Direction selected:** 2026-10-22  
**Product:** ZET — The Outcome-Aware Terminal (descriptor provisional)  
**Status:** visual direction selected; the interface is not implemented.

## Selected image

The approved image mockup is `futuristic_ai_terminal_ci_dashboard.png`, generated in the product-design discussion (1586 × 992 PNG). It depicts a native-looking ZET Mission Control window with a mission rail, central terminal, contextual evidence cards, approvals, checkpoints, verification, an inspector and lightweight status bar.

**Asset tracking:** The image currently lives in the conversation's downloadable artifacts; this markdown is committed to GitHub. The PNG must additionally be copied into `docs/design/assets/zet-flagship-mission-control.png` via a Git checkout; **do not claim the image is in the repository before that binary file has been committed.**

## Vision we are keeping

1. **A real, high-performance terminal** remains central; Rust + wgpu, real PTY, Linux-first, never a browser or Electron replacement.
2. **Outcome-aware workstreams**, connecting observed commands, failures, files, changes, decisions and verified outcomes. Every relationship has honest provenance.
3. **Mission Control**, a distinctive, user-friendly overview of work across sessions and optional external CLI agents, not a new Hermes/OpenClaw.
4. **Expandable context instead of mandatory panels**: mission rail and inspector may collapse independently and terminal focus is always reachable.
5. **Distinctive futuristic craft**: graphite/deep-space, subtle glow, disciplined accent color, crisp typography and compelling but restrained motion.
6. **Safety and verification are real**: render enforcement-backed approval state and exact test/CI evidence, never synthetic data as live results.

## Important: iteration is expected

The approved image is a **design north star**, not a pixel-locked engineering specification. Fine-tune proportions, typography, interaction details, density, motion, theme, mode names, navigation and panel layouts through native prototypes and actual user testing.

Every proposed change is judged on whether it improves usefulness, terminal compatibility, discoverability, trust and responsiveness. The project owner retains approval of major directional changes. Do not use visual fidelity to justify performance regressions, nonfunctional controls, fake metrics or broken shell behavior.

## First implementation experiment

Use the existing `crates/zet/src/window/` composition layer and pure `zet-gpu` renderer. Demonstrate native title/mode affordance, collapsible mission rail, real PTY center and optional inspector populated from actual available state. Do not invent agent/task telemetry. Test terminal focus, resize, output throughput and the transition between layouts, and have an independent auditor review before expansion.

See [Product Charter](../ZET-PRODUCT-CHARTER.md) and [Roadmap](../ROADMAP.md) for product constraints and milestones.
