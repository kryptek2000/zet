# ZET — Flagship UI Reference and Iteration Rules

**Direction selected:** 2026-10-22  
**Product:** ZET — The Outcome-Aware Terminal (descriptor provisional)  
**Status:** visual direction selected; the interface is not implemented.

## Selected image

The approved image mockup is `futuristic_ai_terminal_ci_dashboard.png`, generated in the product-design discussion (1586 × 992 PNG). It depicts a native-looking ZET Mission Control window with a mission rail, central terminal, contextual evidence cards, approvals, checkpoints, verification, an inspector and lightweight status bar.

**Asset tracking:** The original 1586 x 992 PNG was recovered from the owner's
design files on 2026-10-25 and committed at
`docs/design/assets/zet-flagship-mission-control.png`. Verified properties:
1,852,773 bytes, RGB PNG, SHA-256
`7f0c970ff99adbabf770e7efba8719e3e75c6f5e6f9c2f1da784fba5bd60857d`.
See `docs/design/assets/README.md` for provenance.

## Vision we are keeping

1. **A real, high-performance terminal** remains central; Rust + wgpu, real PTY, Linux-first, never a browser or Electron replacement.
2. **Outcome-aware workstreams**, connecting observed commands, failures, files, changes, decisions and verified outcomes. Every relationship has honest provenance.
3. **Mission Control**, a distinctive, user-friendly overview of work across sessions and optional external CLI agents, not a new Hermes/OpenClaw.
4. **Expandable context instead of mandatory panels**: mission rail and inspector may collapse independently and terminal focus is always reachable.
5. **Distinctive futuristic craft**: graphite/deep-space, subtle glow, disciplined accent color, crisp typography and compelling but restrained motion.
6. **Safety and verification are real**: render enforcement-backed approval state and exact test/CI evidence, never synthetic data as live results.

## Signature identity

The approved P2-B direction also includes a distinctive, large, monospace
ASCII-style **ZET** wordmark for the native welcome/startup experience and
empty or introductory workspace states, with optional secondary uses in About
or version surfaces. It is a branding layer around the real terminal, never a
replacement for it: no fake PTY content, no overwriting shell output, no
obstruction of input/scrollback, and no fabricated system data. Exact artwork
and typography require separate owner approval.

See [P2-B — ZET Signature ASCII Identity](P2B-ZET-ASCII-IDENTITY.md) and the
[Roadmap](../ROADMAP.md).

## Important: iteration is expected

The approved image is a **design north star**, not a pixel-locked engineering specification. Fine-tune proportions, typography, interaction details, density, motion, theme, mode names, navigation and panel layouts through native prototypes and actual user testing.

Every proposed change is judged on whether it improves usefulness, terminal compatibility, discoverability, trust and responsiveness. The project owner retains approval of major directional changes. Do not use visual fidelity to justify performance regressions, nonfunctional controls, fake metrics or broken shell behavior.

## First implementation experiment

Use the existing `crates/zet/src/window/` composition layer and pure `zet-gpu` renderer. Demonstrate native title/mode affordance, collapsible mission rail, real PTY center and optional inspector populated from actual available state. Do not invent agent/task telemetry. Test terminal focus, resize, output throughput and the transition between layouts, and have an independent auditor review before expansion.

See [Product Charter](../ZET-PRODUCT-CHARTER.md) and [Roadmap](../ROADMAP.md) for product constraints and milestones.
