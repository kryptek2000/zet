# ZET — Release Acceptance Status

**Updated:** 2026-10-30  
**Authority:** [Product Charter](ZET-PRODUCT-CHARTER.md), [Roadmap](ROADMAP.md), [AGENTS.md](../AGENTS.md)

This document separates four conditions that must not be treated as equivalent:

1. **Source integrated into master** — code present in GitHub master.
2. **Independently audited source** — source reviewed by the assigned independent auditor.
3. **Owner-previewed installed candidate** — a separately built binary launched and visually checked by the owner.
4. **Final release acceptance** — all release gates, including performance and owner approval, are satisfied.

## Current baseline

- GitHub master: `11fa52c36bbd9b4b431f9e34d7cae59f283ff482`
- Exact-head CI: success for run [`41155795648`](https://github.com/kryptek2000/zet/actions/runs/41155795648) on master `11fa52c36bbd9b4b431f9e34d7cae59f283ff482`.
- Selected merged engineering PRs: #35, #39, #40, #42, #43, #48 (header repair), #50 (OpenCode selection), #52 (xterm palette), #53 (PERF-2 instrumentation). Numbers #47, #49 and #51 are closed issues, not these merge PRs.
- Closed issues: #36, #47, #49, #51.
- Open issue: #37 (startup performance on Intel HD 620).

## Acceptance matrix

| Gate | Status | Evidence |
|---|---|---|
| Cold startup < 300 ms | UNVERIFIED | PERF-1 observed internal first-frame median 199 ms (max 258 ms); PERF-2R diagnostic first-output-from-origin median 320 ms (max 551 ms). Different clocks and boundaries; neither proves exec-to-visible-prompt. |
| Warm input-to-pixel < 3 ms | UNVERIFIED | Existing key-to-render CPU timing is not display-presentation timing. No verified end-to-end key-to-pixel release result. |
| Idle RSS < 100 MB | OBSERVED BELOW THRESHOLD; release gate open | PERF-1 release-mode observations: 68,828 KB median (~67.2 MiB); PERF-2R reported 68,400 KB (~66.8 MiB). Measurement methodology still requires owner acceptance for release certification. |
| `cat 100k lines` < 0.9 s | UNVERIFIED | Parser-only deterministic 100,000-line benchmark: 65 ms (independent rerun ~66 ms). Not full PTY-to-display throughput or settled rendering. |

## Performance evidence limits

P37 and merged PERF-2/2R3 instrumentation reports internal startup origin, first terminal feed, frame and CPU-side render-call timing. They do not establish true exec-to-visible-prompt startup or key-to-displayed-pixel latency. Internal first-output is not the same as an observed ready prompt; parser-only throughput is not settled rendering. Debug-build observations are useful for regression spotting only.

Reported startup proxy evidence from PR #43:

- Baseline internal proxy: approximately 179 ms.
- Integrated internal proxy: approximately 204 ms.
- Difference: approximately +25 ms.

These are internal measurements with observed run-to-run variability. They are not true exec-to-visible-prompt cold-start acceptance results.

## Safe A/B acceptance procedure

Baseline source: `7ce2709d09a686638ed35144c9b37d1e2918d740`  
Integrated current master source: `11fa52c36bbd9b4b431f9e34d7cae59f283ff482`

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

## Installed flagship provenance (owner-reported and Beam-verified)

- Canonical launcher: `/home/bandit/.local/bin/zet`
- Latest installed source: `54e0229f9399949cc0606bb893a0efc077e139a7` (color-fidelity merge).
- Installed binary SHA-256: `e04a5555f5b1b3a9fa50cd0761a5745b08b98babc5561192daa12d6a085a991f`.
- Versioned build: `/home/bandit/.local/share/zet/builds/aperture-54e0229-20261030-192839`. Previous build retained for rollback.
- GitHub master `11fa52c...` contains later merged PERF-2 instrumentation; **installed binary is not built from that later master commit**. This documentation change does not install anything.

## Compatibility certification matrix — approved plan, not executed results

The detailed certification program is defined in [ROADMAP.md](ROADMAP.md) under P5. Each row below starts at **NOT RUN** unless independently evidenced; historic focused fixes do not satisfy broad-app certification.

| ID | Required checkpoint | Acceptance status | Evidence/limits |
|---|---|---|---|
| NVIM-1 | Neovim and Neovim-in-tmux: UI, input, colors, splits, resize, alternate screen, Unicode, exit | NOT RUN | E1568-specific issue #36 fixed via PR #40; complete matrix pending. |
| COMPAT-1 P0 | Core shells, editors, multiplexers, AI CLI apps, ANSI/VT protocol, key/paste/resize integrity | NOT RUN | Per-case real application and protocol evidence required. |
| COMPAT-1 P1 | TUIs, Unicode, clipboard/links, SSH, Wayland/X11 and scaling | NOT RUN | Test where available; publish coverage gaps. |
| COMPAT-1 P2 | Advanced graphics protocols (Sixel, Kitty images and similar) | NOT RUN | Conditional; NOT SUPPORTED where verified unsupported and documented. |
| STAB-1 | Long-running multi-tab, editor/agent, output, memory, interruptions, recovery | NOT RUN | Non-destructive supervised tests required. |
| SEC-1 | Escape handling, content isolation, redaction, recorded-data integrity, provenance safety | NOT RUN | Existing regression tests are not certification of the full program. |

Status vocabulary: **PASS / FAIL / BLOCKED / NOT RUN / NOT SUPPORTED**. Every assessed case needs application version, ZET source/build identity, environment, reproduction steps, raw evidence, independent audit and owner acceptance. Mandatory P0 failures block release; conditional P2 advanced graphics are not mandatory for Linux v1. No invented blanket compatibility claims.

## Remaining release blockers

- Cold startup < 300 ms: UNVERIFIED.
- Warm input-to-pixel < 3 ms: UNVERIFIED.
- Idle RSS < 100 MB: observed below threshold; release certification open pending accepted method.
- `cat 100k lines` < 0.9 s: UNVERIFIED.
- Previous flagship visuals were owner-accepted; final release approval remains open.
- NVIM-1, applicable COMPAT-1 P0, STAB-1 and SEC-1 certification: NOT RUN. P1 limitations must be disclosed; P2 advanced graphics are conditional.
- P3-A1 recorded command history and bounded output inspection are integrated into master.
