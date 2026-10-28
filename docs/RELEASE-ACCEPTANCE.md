# ZET — Release Acceptance Status

**Updated:** 2026-10-28  
**Authority:** [Product Charter](ZET-PRODUCT-CHARTER.md), [Roadmap](ROADMAP.md), [AGENTS.md](../AGENTS.md)

This document separates four conditions that must not be treated as equivalent:

1. **Source integrated into master** — code present in GitHub master.
2. **Independently audited source** — source reviewed by the assigned independent auditor.
3. **Owner-previewed installed candidate** — a separately built binary launched and visually checked by the owner.
4. **Final release acceptance** — all release gates, including performance and owner approval, are satisfied.

## Current baseline

- GitHub master: `91d409bd521a5127c2c14d22bd9ecc1513d2ba7b`
- Exact-head CI: success for run `40495565730` on master.
- Merged PRs: #35, #39, #40, #42, #43.
- Closed issue: #36.
- Open issue: #37 (startup performance on Intel HD 620).

## Acceptance matrix

| Gate | Status | Evidence |
|---|---|---|
| Cold startup < 300 ms | UNVERIFIED | Only debug first-frame proxy measurements (245–332 ms) are recorded; they are noisy and are not exec-to-visible-prompt release evidence. |
| Warm input-to-pixel < 3 ms | UNVERIFIED | One debug sample of 3.058 ms exists; no release measurement or input-to-pixel instrumentation result is available. |
| Idle RSS < 100 MB | UNVERIFIED (debug observation below gate) | Debug P2-B1 comparison observed 92,784 KB idle RSS, but release-build idle RSS is not recorded. |
| `cat 100k lines` < 0.9 s | UNVERIFIED | Throughput was not measured in the recorded evidence. |

## Performance evidence limits

Existing P37 instrumentation reports internal CPU/GPU stage timings. Those markers do not establish true exec-to-visible-prompt startup latency or input-to-pixel latency. Debug-build observations are useful for regression spotting only.

## Safe A/B acceptance procedure

Baseline source: `7ce2709d09a686638ed35144c9b37d1e2918d740`  
Integrated candidate source: `91d409bd521a5127c2c14d22bd9ecc1513d2ba7b`

1. Confirm both source identities before any comparison.
2. Prefer existing valid release artifacts; otherwise build sequentially with `CARGO_BUILD_JOBS=2`.
3. Use isolated `ZET_HOME` (or `HOME`) directories for each build.
4. Use equal window size, display scale, compositor state and launch conditions.
5. Record a small bounded number of cold-start observations per build.
6. Record idle RSS after a fixed stabilization period.
7. Record warm input latency only if input-to-pixel instrumentation is available.
8. Record `cat 100k` elapsed time separately.
9. Do not kill unrelated processes, restart Hyprland, inject input automatically, or replace the installed binary.

This procedure can establish cold-start timing and idle RSS when measured correctly. It cannot establish true visual presentation from internal CPU markers; warm input-to-pixel latency requires dedicated instrumentation.

## Remaining release blockers

- Cold startup < 300 ms: UNVERIFIED.
- Warm input-to-pixel < 3 ms: UNVERIFIED.
- Idle RSS < 100 MB: UNVERIFIED for release.
- `cat 100k lines` < 0.9 s: UNVERIFIED.
- Owner visual acceptance and final release approval remain open.
- P3 implementation is not part of this release checkpoint.
