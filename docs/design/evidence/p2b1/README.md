# P2-B1 — Native UI Evidence

**Captured:** 2026-10-25
**Baseline:** `d9bca1bc10a3e3c232a0b13b80b12701ddb5265b`
**Branch:** `work/p2b1-native-ui-slice`
**Reference:** [`../../assets/zet-flagship-mission-control.png`](../../assets/zet-flagship-mission-control.png)

These are genuine ZET captures from a real Linux Wayland compositor
(Hyprland 0.56.2) running a real bash PTY. They are not HTML mockups, browser
renders, generated images or manually painted screenshots.

## Capture method and limitation

The owner's physical desktop session was locked during capture. Images 02 and
03 were taken on Hyprland headless outputs. Images 01, 04, 05 and 06 were taken
inside a nested Hyprland session (`WAYLAND-1`, 1920 x 1080 and 1024 x 768,
scale 1) so the lock screen did not obscure the application. Images 01 and 04
are the matched before/after pair from the same nested environment and the
same three-tab, one-command state. All paths use the same native wgpu
application and the same real PTY path.

The faint wallpaper visible in the baseline captures is Omarchy's default
window opacity, not ZET content. The after captures are the application surface
without the outer desktop bar; the active tab bar is at the top of the image.

## Before / after

| File | Size | What it shows |
|---|---|---|
| `01-baseline-workspace-rail-1920x1080.png` | 1920 x 1080 | Matched pre-change rail: text-only sections, single-row session rows, basic selected fill, three real tabs and one recorded command. |
| `02-baseline-workspace-rail-1024x768.png` | 1024 x 768 | Pre-change narrow rail with the same structure. |
| `03-baseline-terminal-focus-1920x1080.png` | 1920 x 1080 | Pre-change Terminal Focus. |
| `04-after-workspace-rail.png` | 1920 x 1080 | Matched post-change pair for 01: two-row session cards, gaps, active card, section counts, observed command row, panel rule and footer strip; three real tabs. |
| `05-after-workspace-rail-1024x768.png` | 1024 x 768 | Same rail at the narrow width; all three session cards, tabs and footer remain readable. |
| `06-after-terminal-focus-1920x1080.png` | 1920 x 1080 | Post-change Terminal Focus; active-tab accent and separators, terminal dominant. |
| `07-compat-tmux.png` | 1920 x 1080 | Real `tmux 3.8` in the PTY; rail reports the real running state and recorded `tmux` command. |
| `08-compat-nvim-after.png` | 1920 x 1080 | Real `NVIM v0.12.6`; pre-existing DSR/background-query failure on the after branch. |
| `09-compat-nvim-master.png` | 1920 x 1080 | Same Neovim failure on master, proving it is not introduced by P2-B1. |

The after images show only the temporary bash session used for verification:
the prompt, `pwd` output and the recorded `pwd` command are real. No mission,
CI, agent, approval, checkpoint or verification data is displayed.

## Verification

`SHA256SUMS` records every PNG in this directory. The reference checksum and
provenance are in [`REFERENCE-PROVENANCE.md`](REFERENCE-PROVENANCE.md); the
debug-build measurements and their caveats are in
[`PERFORMANCE.md`](PERFORMANCE.md); the tmux/Neovim smoke test and the
pre-existing DSR limitation are in [`COMPATIBILITY.md`](COMPATIBILITY.md).

Focused code checks and the full exact-head CI run are recorded in the P2-B1
draft PR. This evidence directory is committed before the independent audit;
the author does not self-approve visual fidelity.
