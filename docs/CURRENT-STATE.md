# ZET — Current Development State

**Updated:** 2026-10-27  
**Authority:** [Product Charter](ZET-PRODUCT-CHARTER.md), [Roadmap](ROADMAP.md)

## Verified Git state

- Remote master: `91d409bd521a5127c2c14d22bd9ecc1513d2ba7b`
- Local master: `91d409bd521a5127c2c14d22bd9ecc1513d2ba7b`
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
- P2-B1 release-performance acceptance remains open; PR #35 has been merged into master.
- Initial owner visual approval of the integrated candidate has been received; final merge/release approval remains with the owner.

## Current next milestone

P3-A0 — Evidence-linked commands contract complete.

The bounded P3-A1 contract is now documented in `docs/design/P3-A1-EVIDENCE-LINKED-COMMANDS-CONTRACT.md`. No P3 implementation is authorized yet.

## Open work

- P3-A0 documentation and architecture checkpoint prepared; awaiting architect review.

- Independent audit of the integrated flagship candidate.
- Owner visual acceptance.
- Release-mode performance acceptance.
- Neovim E1568 compatibility claim resolution (Issue #36).
- P2-B ASCII identity remains planned, not implemented.
