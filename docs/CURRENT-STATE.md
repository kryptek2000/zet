# ZET — Current Development State

**Updated:** 2026-10-30  
**Authority:** [Product Charter](ZET-PRODUCT-CHARTER.md), [Roadmap](ROADMAP.md)

## Verified Git state

- Remote master: `11fa52c36bbd9b4b431f9e34d7cae59f283ff482`
- Local master: `11fa52c36bbd9b4b431f9e34d7cae59f283ff482`
- Canonical installed ZET source: `79a46069607f7b04bca0f8fb7d7d5f9ef98e6248`
- Installed binary: `/home/bandit/.local/bin/zet`
- Installed binary SHA-256: `812bb118a5bd050bb3370482b2cda950640913fbc1c2cb0dce587e0561be65a5`
- The installed binary is a separately built integrated candidate from source `79a46069607f7b04bca0f8fb7d7d5f9ef98e6248` (SHA-256 `812bb118a5bd050bb3370482b2cda950640913fbc1c2cb0dce587e0561be65a5`). It is last-reported installation provenance, not proof that it matches current GitHub master.

## Completed flagship work

- Native terminal and PTY foundation.
- Tabs and shell interaction.
- Native Mission Rail with P2-B1 visual improvements.
- Contextual Inspector foundation with independent visibility, selection and session identity.
- Three-panel native layout with independent rail/Inspector toggles.
- Recorded-command Inspector and bounded output inspection (P3-A1).
- Additive performance instrumentation.
- Wayland clipboard paste support.
- Neovim background-color query repair.

## Outstanding release gates

- Cold startup < 300 ms: UNVERIFIED.
- Warm input-to-pixel latency < 3 ms: UNVERIFIED.
- Idle RSS < 100 MB: PASS in a reported debug comparison, but final release acceptance remains UNVERIFIED.
- `cat 100k lines` < 0.9 s: UNVERIFIED.
- PRs #35, #39, #40, #42 and #43 are merged into master.
- Owner visual approval of the integrated candidate was received; final release approval remains open.

## Release acceptance

See [RELEASE-ACCEPTANCE.md](RELEASE-ACCEPTANCE.md) for the separated source/audit/preview/release status and the performance acceptance matrix.

## Current next milestone

P3 — Evidence-linked commands and mission timeline.

P3-A1 observed command history and bounded output inspection are integrated into master. Further P3 work requires an architect-approved contract.

## Open work

- Owner visual acceptance.
- Release-mode performance acceptance.
- Issues #36, #47, #49 and #51 are closed.
- Issue #37 remains open.
- P2-B ASCII identity remains planned, not implemented.
