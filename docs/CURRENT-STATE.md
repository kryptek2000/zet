# ZET — Current Development State

**Updated:** 2026-10-30  
**Authority:** [Product Charter](ZET-PRODUCT-CHARTER.md), [Roadmap](ROADMAP.md)

## Verified Git state

- Remote master: `11fa52c36bbd9b4b431f9e34d7cae59f283ff482`
- Local master: `11fa52c36bbd9b4b431f9e34d7cae59f283ff482`
- Canonical installed ZET source: `54e0229f9399949cc0606bb893a0efc077e139a7` (owner-reported, Beam-verified)
- Installed binary: `/home/bandit/.local/bin/zet`
- Installed binary SHA-256: `e04a5555f5b1b3a9fa50cd0761a5745b08b98babc5561192daa12d6a085a991f`
- Installed versioned build: `/home/bandit/.local/share/zet/builds/aperture-54e0229-20261030-192839`; rollback preserved. Current master `11fa52c...` contains newer PERF-2 instrumentation and is **not** the installed binary source. Installation state is last-reported host evidence, not remotely verifiable from GitHub.

## Completed flagship work

- Native terminal and PTY foundation.
- Tabs and shell interaction.
- Native Mission Rail with P2-B1 visual improvements.
- Contextual Inspector foundation with independent visibility, selection and session identity.
- Three-panel native layout with independent rail/Inspector toggles.
- Recorded-command Inspector and bounded output inspection (P3-A1).
- Additive PERF-2 instrumentation (PR #53) merged into master; release performance certification remains separate.
- Wayland clipboard paste support.
- Neovim background-color query repair.

## Outstanding release gates

- Cold startup < 300 ms: UNVERIFIED.
- Warm input-to-pixel latency < 3 ms: UNVERIFIED.
- Idle RSS < 100 MB: measured below threshold in PERF-1 (~67.2 MiB median) and PERF-2R (~66.8 MiB median); final release acceptance depends on approved measurement method.
- `cat 100k lines` < 0.9 s: UNVERIFIED.
- PRs #35, #39, #40, #42, #43, #48, #50, #52 and #53 are merged into master.
- Owner visual approval of the integrated candidate was received; final release approval remains open.

## Release acceptance

See [RELEASE-ACCEPTANCE.md](RELEASE-ACCEPTANCE.md) for the separated source/audit/preview/release status and the performance acceptance matrix.

## Current next milestone

P3 — Evidence-linked commands and mission timeline.

P3-A1 observed command history and bounded output inspection are integrated into master. Further P3 work requires an architect-approved contract.

## Open work

- Final public release acceptance (prior installed flagship visual acceptance was recorded).
- Release-mode performance acceptance.
- Issues #36, #47, #49 and #51 are closed.
- Issue #37 remains open.
- NVIM-1, COMPAT-1, STAB-1 and SEC-1 are owner-approved P5/P6 certification checkpoints; all full-certification cases are NOT RUN pending evidence.
- P2-B ASCII identity remains planned, not implemented.
