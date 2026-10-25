# Issue #22 — native re-test of the rail width fix

Captured by the Implementor on 2026-10-24 for PR #28, after the independent
code audit passed and the native visual audit reported the duration-unit
clipping defect.

## Provenance

- Feature commit: `1003eeb2dd3eb7651ce490399104729c3a4068d3`
  (`fix(zet): budget rail rows by terminal cells`, PR #28 branch
  `feat/p2a-slice-4b-mission-rail`).
- Source tree: `/home/bandit/Projects/zet`, built with
  `cargo build -p zet`; the running binary is
  `target/debug/zet`.
- Isolated environment: `HOME=/tmp/opencode/zet-repair/home`,
  `ZET_HOME=/tmp/opencode/zet-repair/zet`. The shell-integration hook recorded
  in the PTY setup line resolves to `/home/bandit/Projects/zet/shell/`.
- Compositor: Hyprland 0.56.2 on Wayland. Monitor `eDP-1`
  (1920x1080@60, scale 1.5); the window is fullscreen at 1280x720 logical.
- Capture: `grim -g "<window geometry>"`, genuine compositor output. The
  `*-zoom-3x.png` files are nearest-neighbour crops of the corresponding
  full capture (no redrawing, no mockups).
- The screenshots were taken from the feature-commit build, not from a
  worktree with later edits. The only later repository change is this
  evidence directory.

## Files

| File | What it shows |
|---|---|
| `01-terminal-focus.png` | Terminal Focus after the fix: rail hidden, full-width grid, three real sessions in the tab bar |
| `02-terminal-focus-long-history.png` | Terminal Focus with the long recorded command and its output |
| `03-workspace-rail.png` | Workspace rail after the fix: three sessions, eight observed commands, every duration unit present |
| `03-workspace-rail-zoom-3x.png` | 3x crop of the rail above; `printf 'zet-nati… 0 41ms`, `sleep 1.234 0 1237ms` |
| `04-long-duration-rail-zoom-3x.png` | 3x crop of the earlier rail state; includes exit `127` and duration `1222ms`, plus the long `printf … sleep 0.02` command rendered as `0 24ms` |
| `05-narrow-panel-refused.png` | Narrow window: rail refused; `stty size` shows `48 98` with the rail and `48 60` after the rail is refused |
| `06-search-with-rail.png` | Search bar and hit highlight with a nonzero terminal origin (`vp.x = 24`); it does not paint over the rail |
| `07-ghost-with-rail.png` | History ghost suffix inside the terminal viewport with the rail open |
| `08-multi-session-rail.png` | Three sessions with long labels: session 1 `…session-label`, tab 3 cwd `window`; footer `cwd …ates/zet/src/window` |
| `block-store.txt` | Exact rows read from the isolated block store during the session |
| `SHA256SUMS` | SHA-256 of every PNG in this directory |

## Measured facts

- Rail width: 24 terminal columns.
- With the rail open, the PTY at 900x720 logical was `48 98`; when the
  window was narrowed to 440x720 logical the panel was refused and the PTY
  became `48 60`. The rail disappears rather than narrowing the terminal
  below its minimum.
- Observed command rows from `block-store.txt`:
  - `printf 'zet-native-repair-ok\n' && sleep 0.02` — exit `0`, `24 ms`
  - `command this-command-does-not-exist-zet-audit` — exit `127`, `7 ms`
  - `sleep 0.123` — exit `0`, `111 ms`
  - `ls /nonexistent-zet-audit-path` — exit `2`, `2 ms`
  - `sleep 1.234` — exit `0`, `1222 ms`
  - `printf 'zet-native-repair-final\n' && sleep 0.01` — exit `0`, `41 ms`
  - `sleep 1.234` — exit `0`, `1237 ms`
- The rail screenshots were taken while the active session's cwd was
  `/home/bandit/Projects/zet/crates/zet/src/window` and session 1 was in
  `/tmp/opencode/zet-repair/a-very-long-directory-name-for-session-label`,
  so the session label and footer are both exercising the long-label paths.

## What this demonstrates

The confirmed defect was a duration unit clipped by the 24-column rail:
`14ms` painted as `14`. With the fix, every row in
`03-workspace-rail-zoom-3x.png` and `04-long-duration-rail-zoom-3x.png`
contains a complete duration field, including multi-digit values and
`1237ms`; truncated command names end in `…`; no row extends past the rail
boundary. The long session label and CWD footer keep their most specific
right-hand text.

## Remaining unverified native checks

- **Real mouse focus and session selection:** not verified natively in this
  capture. The desktop exposes no pointer-injection tool (`ydotool`,
  `wlrctl`, and XTest are unavailable; `/dev/uinput` is root-only and the
  session has no passwordless sudo). Hyprland's `send_shortcut` mouse path
  did not produce a focus or selection change. The routing is covered by
  the existing production-path unit tests, but this is not claimed as a
  native PASS.
- **Scrollback wheel traversal:** not verified natively; the same pointer
  injection limitation prevents wheel events. Search (which walks scrollback
  hits) is shown in `06-search-with-rail.png`.
- No GPU readback or synthetic framebuffer was used. No mockups.
