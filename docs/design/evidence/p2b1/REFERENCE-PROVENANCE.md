# P2-B1 — Flagship Reference Provenance

**Date:** 2026-10-25
**Branch:** `work/p2b1-native-ui-slice`
**Baseline:** `d9bca1bc10a3e3c232a0b13b80b12701ddb5265b`
**Issue:** [#34](https://github.com/kryptek2000/zet/issues/34)

## Verified asset

| Property | Value |
|---|---|
| Original filename | `futuristic_ai_terminal_ci_dashboard.png` |
| Repository path | `docs/design/assets/zet-flagship-mission-control.png` |
| Dimensions | 1586 x 992 |
| Format | RGB PNG, 8-bit, non-interlaced |
| Bytes | 1,852,773 |
| SHA-256 | `7f0c970ff99adbabf770e7efba8719e3e75c6f5e6f9c2f1da784fba5bd60857d` |
| Approved | 2026-10-22 |
| Recovered | 2026-10-25 by GPT-6 from the owner's original design files |

The local file was checked against the expected dimensions, byte count and
SHA-256 before being copied unchanged into `docs/design/assets/`. The asset is
a design north star, not a pixel-locked engineering specification and not a
screenshot of the shipped application. Its mission, CI, agent and verification
content is concept data and must never be presented as live ZET state.

## Recovery history

The first recovery pass on 2026-10-25 did not find the exact file in the
repository, Git history, all local clones/worktrees, the accessible filesystem,
Chrome caches, clipboard image cache, OpenCode/Hermes session attachments, or
GitHub issue/PR comments. The exact file was subsequently recovered by GPT-6
from the owner's design files and transferred to the owner's Downloads folder.
That blocker is resolved.

Candidate images examined during the first pass and rejected as substitutes:

| File | Dimensions | Why it is not the approved reference |
|---|---|---|
| `Downloads/ZET Neon Mission Control Terminal.png` | 1536 x 1024 | Later (2026-10-24) generated concept; no inspector/evidence/verification surfaces described for the approved image. |
| `Downloads/neon_zet_over_a_reflective_planetary_horizon.png` | 1672 x 941 | README planetary-horizon marketing hero; explicitly not a substitute. |
| `zet/docs/design/assets/zet-readme-hero-primary.png` | 1672 x 941 | Committed README hero; same artwork as above. |
| `Downloads/Zet Terminal UI Mockup.html` | n/a (HTML) | Synthetic-data HTML concept dated 2026-10-21/22, not the approved 1586 x 992 binary. |

## Baseline screenshots

`docs/design/evidence/p2b1/01-*` through `03-*` are genuine compositor captures
of the pre-change application. They were taken on a Hyprland 0.56.2 headless
output at 1920 x 1080 and 1024 x 768, scale 1, from the master-equivalent UI
code. The physical desktop session was locked, so these are headless-output
captures rather than photographs of the owner's physical display; they are
still real ZET rendering through a real Wayland compositor, not fabricated
mockups. The faint wallpaper visible in some captures is Omarchy's default
window opacity, not ZET content.
