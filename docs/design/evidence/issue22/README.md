# Issue #22 — native acceptance evidence (Slice 4B Mission Rail)

Captured by the Independent Auditor on 2026-10-24 for PR #28.

## Provenance

- Feature HEAD: `e81d0ad5b4f037ff29b094119d8d93a1ba2d864f`
  (`feat/p2a-slice-4b-mission-rail`)
- Source tree: `/tmp/opencode/zet-slice4b` (clean), built to
  `CARGO_TARGET_DIR=/home/bandit/.cache/zet-slice4b-target`.
- Binary: `crates/zet` debug build. The running binary is tied to that tree
  by the compile-time `CARGO_MANIFEST_DIR` path that appears in the PTY setup
  line (`source /tmp/opencode/zet-slice4b/crates/zet/../../shell/zet-integration.bash`)
  recorded in the isolated `HOME/.bash_history`.
- Isolated environment: `HOME=/tmp/zet-audit/home`, `ZET_HOME=/tmp/zet-audit/zet`.
- Compositor: Hyprland 0.56.2 on Wayland, `grim` region capture at 1.5x scale.

## Files

| File | What it shows |
|---|---|
| `01-terminal-focus.png` | Default TerminalFocus: full-width grid, no rail |
| `02-terminal-focus-output.png` | TerminalFocus after three real recorded commands |
| `03-workspace-rail.png` | Rail visible; sessions + OBSERVED COMMANDS + footer |
| `03-rail-zoom.png` | 4x zoom of the rail (shows command-row clipping) |
| `04-search-rail.png` | Search hits with nonzero viewport origin (`vp.x = 24`) |
| `05-ghost-rail.png` | History ghost suffix inside the terminal viewport |
| `06-select-all-rail.png` | Select-all tint stopping at the rail boundary |
| `07-three-tabs-rail.png` | Three tabs: tab bar after the rail, three sessions |
| `07-top-zoom.png` | 1.8x zoom of tab bar + rail header alignment |
| `10-toggle-off.png` | Rail toggled off; full-width tab bar; content preserved |
| `11-stty-rail-off.png` | `stty size` → `44 84` with rail hidden |
| `12-stty-rail-on.png` | `stty size` → `44 60` with rail visible |
| `implementor-terminal-focus.png` | Implementor capture (sha256 `50ee0d03…`) |
| `implementor-mission-rail.png` | Implementor capture (sha256 `1ecdaf9c…`) |

## Measured facts

- Rail width = 24 columns. PTY columns drop 84 → 60 when the rail opens,
  exactly `RAIL_COLS`; rows stay 44 in both states.
- Real block store recorded genuine rows with exit codes and durations:
  `echo hello-from-zet-audit` (0, 14 ms), `ls -la /tmp/zet-audit` (0, 5 ms),
  `printf …` (0, 2 ms).
- Idle CPU with rail visible, three tabs: 10 ticks / 10 s = **1.0 % of one core**
  (debug build; 250 ms heartbeat).
- RSS while running: **95.7 MB** (debug build, unoptimised).

## Known visual defect captured here

`03-rail-zoom.png` and `03-workspace-rail.png` show the OBSERVED COMMANDS rows
clipping the duration unit. A command truncated to 18 chars plus exit code and
`NNms` exceeds the 24-column rail, so `14ms` renders as `14`. Short commands
(`pwd 0 6ms`) fit. See the audit report for severity.
