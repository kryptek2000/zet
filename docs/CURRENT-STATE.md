# ZET — Current Development State

**Updated:** 2026-10-28  
**Authority:** [Product Charter](ZET-PRODUCT-CHARTER.md), [Roadmap](ROADMAP.md)

## Verified Git state

- Remote master: `6ed65954b08053f37999459d8f48f3b495b46b67`
- Local master: `6ed65954b08053f37999459d8f48f3b495b46b67`
- Canonical installed ZET source: `79a46069607f7b04bca0f8fb7d7d5f9ef98e6248`
- Installed binary: `/home/bandit/.local/bin/zet`
- Installed binary SHA-256: `812bb118a5bd050bb3370482b2cda950640913fbc1c2cb0dce587e0561be65a5`
- The installed binary is a separately built integrated candidate. It was not rebuilt from the merge commit.

## Completed flagship work

- Native terminal and PTY foundation.
- Tabs and shell interaction.
- Native Mission Rail with P2-B1 visual improvements.
- Contextual Inspector foundation with independent visibility, selection and session identity.
- Three-panel native layout with independent rail/Inspector toggles.
- Existing recorded-command functionality.
- Additive performance instrumentation.
- Wayland clipboard paste support.
- Neovim background-color query repair.

## Outstanding release gates

- Cold startup < 300 ms: UNVERIFIED.
- Warm input-to-pixel latency < 3 ms: UNVERIFIED.
- Idle RSS < 100 MB: PASS in reported comparison, but final release acceptance remains open.
- `cat 100k lines` < 0.9 s: UNVERIFIED.
- PR #35 performance hold remains open.
- Owner visual approval remains required for final merge/release.

## Release acceptance

See [RELEASE-ACCEPTANCE.md](RELEASE-ACCEPTANCE.md) for the separated source/audit/preview/release status and the performance acceptance matrix.

## Current next milestone

P3 — Evidence-linked commands and mission timeline.

Begin with an architect-approved bounded contract using existing command recording and provenance. No implementation is authorized by this documentation sync.

## Open work

- Independent audit of the integrated flagship candidate.
- Owner visual acceptance.
- Release-mode performance acceptance.
- Neovim E1568 compatibility claim resolution (Issue #36).
- P2-B ASCII identity remains planned, not implemented.
