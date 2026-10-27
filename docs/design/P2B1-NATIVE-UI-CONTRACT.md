# P2-B1 — Native Mission Rail, Tabs and Chrome Contract

**Status:** implementation contract for the first P2-B1 native slice. The rail,
tab and chrome changes described in §5–§8 are implemented on
`work/p2b1-native-ui-slice`; the Inspector, status bar and ASCII welcome
overlay are explicitly out of scope.
**Tracking:** [Issue #34](https://github.com/kryptek2000/zet/issues/34)
**Parent:** [Issue #29](https://github.com/kryptek2000/zet/issues/29)
**Baseline:** `d9bca1bc10a3e3c232a0b13b80b12701ddb5265b`
**Approved reference:** [`docs/design/assets/zet-flagship-mission-control.png`](assets/zet-flagship-mission-control.png)
(1586 x 992, SHA-256 `7f0c970ff99adbabf770e7efba8719e3e75c6f5e6f9c2f1da784fba5bd60857d`).

The reference is a visual north star, not a pixel-locked specification. It
contains synthetic mission, CI and agent data. None of that content is copied
into the application; only the visual language and hierarchy are used with
real ZET state.

## 1. Shipped versus planned

| Surface | This slice | Notes |
|---|---|---|
| Mission Rail hierarchy, cards, selected/active/focused states | **Shipped** | Two-row session cards, real cwd/state, active marker, rail focus styling. |
| Rail collapse/expand and narrow-window behavior | **Shipped** | Existing `Ctrl+Shift+M` toggle; compact row layout below 16 rows; PTY resized to the exact viewport. |
| Tab bar active/inactive styling and separators | **Shipped** | Active accent cell, active fill, inactive separators, white/dim ink. |
| Rail/terminal panel rule and footer strip | **Shipped** | One-cell vertical rule; no new rows. |
| Inspector / contextual evidence panel | **Not in this slice** | Requires its own approved contract. |
| Status bar with model/GPU/context telemetry | **Not in this slice** | No new row; no fabricated telemetry. |
| ASCII ZET welcome wordmark | **Not in this slice** | Separate owner approval required. |
| Mission graph, approvals, checkpoints, verification receipts | **Not in this slice** | No synthetic data; these surfaces wait for real source data. |
| Hover styling for rail cards | **Defined, not implemented** | Requires pointer-hover tracking; deferred to a later bounded change. |

## 2. Composition and proportions

```
┌─────────────────────────────────────────────────────────────┐
│ tab bar (row 0, only when tabs > 1)                         │
├──────────────┬──────────────────────────────────────────────┤
│ Mission Rail │ real terminal grid                           │
│ 24 cells     │ remaining columns                            │
│ + 1-cell rule│ exact PTY viewport                           │
├──────────────┴──────────────────────────────────────────────┤
│ (no dedicated status row; rail footer is the only strip)     │
└─────────────────────────────────────────────────────────────┘
```

- Rail default: `RAIL_COLS = 24`; the rightmost cell is the panel rule, so
  content uses 23 columns.
- Terminal minimum: `MIN_TERM_COLS = 40`, `MIN_TERM_ROWS = 10`.
- The tab bar steals row 0 only when there is more than one tab. There is no
  new permanent top or bottom row.
- The Inspector is absent. The terminal remains the dominant surface at every
  supported width.

## 3. Surfaces, ink and accents

All chrome colors are linear-light `Rgba` constants in
`crates/zet-gpu/src/renderer.rs`. The table lists the display hex for designers;
the renderer writes the linear values, which the sRGB swapchain encodes.

| Token | Display hex | Role |
|---|---|---|
| `RAIL_BG` | `#1d2432` | Rail panel surface, one step above the terminal's near-black grid. |
| `RAIL_BORDER` | `#4b596f` | Rail/terminal panel rule. |
| `RAIL_SELECTED_BG` | `#384863` | Selected rail card while the rail owns focus. |
| `RAIL_ACTIVE_BG` | `#293548` | Active session card while the terminal owns focus. |
| `RAIL_HEADER_INK` | `#ffffff` | Brand and active session name. |
| `RAIL_SECTION_INK` | `#c8d4e7` | Section labels (`SESSIONS`, `OBSERVED COMMANDS`). |
| `RAIL_TEXT_INK` | `#e2eaf6` | Session names and command text. |
| `RAIL_DIM_INK` | `#aab6c8` | Paths, counts and secondary detail. |
| `RAIL_ACCENT` | `#6fd1e5` | Active/focused marker and active tab accent. |
| `RAIL_FOCUS` | `#95eaf6` | Brighter marker only while the rail owns keyboard focus. |
| `RAIL_OK` | `#76ddbf` | Live/running session state. |
| `RAIL_BAD` | `#ef9595` | Exited/failed session or command state. |
| `RAIL_FOOTER_BG` | `#161d29` | Rail footer strip. |
| `RAIL_FOOTER_INK` | `#adbcce` | Footer cwd text. |
| `TABBAR_BG` | `#242d3c` | Inactive tab bar surface. |
| `TAB_ACTIVE_BG` | `#3c4d6c` | Active tab surface. |
| `TAB_ACTIVE_ACCENT` | `#6fd1e5` | One-cell active tab marker. |
| `TAB_SEPARATOR` | `#3f4b5d` | One-cell separator at inactive segment starts. |
| `TAB_DIM_INK` | `#b3b8c4` | Inactive tab label. |

Rules:

- Cyan/teal is the working accent for active, selected and healthy states.
- Red is used only for a real nonzero exit or an exited session. It is never
  decorative.
- Violet is reserved for future real agent surfaces; this slice does not use it.
- No glow, gradient, blur, shadow or rounded corner is invented in the
  cell-quantized renderer. Depth comes from three surface steps and the
  one-cell rule.

Contrast is tested in `crates/zet-gpu/src/renderer.rs`:
`chrome_tokens_meet_text_and_marker_contrast`. Text pairs are at least 4.5:1;
marker/focus pairs are at least 3:1 in the current tokens.

## 4. Typography and rendering constraints

- One monospace terminal grid; no proportional UI font is added in this slice.
  The reference's sans-serif labels are approximated with monospace capitals,
  spacing and ink hierarchy rather than a second font dependency.
- Every printable glyph consumes at least one cell; wide/emoji glyphs consume
  two. Chrome uses a small vocabulary: `A–Z`, `0–9`, space, `/`, `~`, `.`, `-`,
  `:`, `·`, `›`, `▸`, `●`, `○`, `×`, and `…`.
- If a glyph is missing from the atlas it must degrade to a safe textual label,
  never to a fabricated icon or control.
- Labels are truncated by display cells, not bytes. Session paths keep their
  most specific tail; commands reserve duration/exit cells before truncating.
- Header hierarchy is: brand (`ZET`) → mode (`WORKSPACE`) → section labels →
  session names → detail. No extra font sizes or weights are available.

## 5. Mission Rail anatomy

### 5.1 Expanded layout (`viewport_rows >= 16`)

| Row | Content |
|---|---|
| 0 | Accent block + `ZET` brand |
| 1 | `WORKSPACE` mode label |
| 2 | blank spacer |
| 3 | `SESSIONS` + real session count, right-aligned |
| 4… | Two-row session cards, one blank row between cards |
| next | `OBSERVED COMMANDS` + real recorded-command count |
| next | One row per real recorded command, up to the row budget |
| last | Footer strip: `cwd <shortened path>` |

### 5.2 Compact layout (`viewport_rows < 16`)

The mode row and inter-card gap are dropped so at least two real sessions remain
visible at the 10-row terminal minimum. Session cards stay two rows. Counts in
the section headers make omitted overflow explicit; the rail never invents a
row for a session that does not exist.

### 5.3 Session card

```
▸ ● 3 api              title row: marker, state dot, index, cwd tail
  input · ~/zet/api    detail row: real state + shortened real cwd
```

- The active session has a `▸` marker and a one-cell accent bar.
- The state dot is `●` for running, `○` for prompt/input, `×` for exited.
  The state text is always present as well, so color is not the only cue.
- An exited session shows `exit <code>` when a real code exists.
- The selected card (keyboard/mouse rail focus) gets the selected fill and a
  brighter focus bar. When the terminal owns focus, the active card is filled;
  a stale `rail_sel` never looks like the active session.
- No card is shown for a session that does not exist. The count is the real
  number of tabs.

### 5.4 Commands

- `OBSERVED COMMANDS` is populated only from the active tab's real recorder.
- Each row starts with `›` (success/unknown) or `×` (real nonzero exit), then
  the recorded command, exit code and duration.
- Durations and exit codes are reserved before command truncation, so a long
  command never hides them.
- The section is omitted when there are no recorded commands.

### 5.5 Footer

- Shows `cwd <path>` with `$HOME` shortened to `~`; unknown stays `—`.
- The footer is a darker strip, not a new row outside the rail.
- No model, GPU, context, token or CI telemetry is invented.

## 6. Native tabs

- The tab bar uses row 0 only when `tabs > 1`.
- Segments are equal-width across the drawable strip after the rail. The rail
  region is never painted as a tab control and never hit-tests as one.
- Every segment's first cell is a state block: accent for the active tab,
  separator for inactive tabs. The label's leading padding cell is that block.
- Active tab: `TAB_ACTIVE_BG`, white ink, `TAB_ACTIVE_ACCENT` block.
- Inactive tab: `TABBAR_BG`, dim ink, `TAB_SEPARATOR` block.
- Labels are `index + cwd tail`; no fake close/plus buttons are added. Closing
  remains `Ctrl+Shift+W`; new tabs remain `Ctrl+Shift+T`.
- Clicking a tab still switches the active tab and returns focus to the
  terminal; the rail selection follows the active tab.

## 7. Chrome, borders and depth

- The rail's rightmost cell is a one-cell `RAIL_BORDER` rule from the tab bar
  to the bottom. It is the only permanent vertical separator.
- The footer uses `RAIL_FOOTER_BG` and spans the rail content width.
- No extra title bar, status row, rounded window chrome, drop shadow or
  decorative icon strip is added. The OS/Wayland window decoration remains the
  outer chrome.
- The tab bar is the top chrome when tabs exist; in single-tab Terminal Focus
  the terminal owns row 0 and the rail starts at row 0.

## 8. States

| State | Treatment | Truth source |
|---|---|---|
| Active session | Accent bar, `▸`, bright name, active fill in terminal focus | `snapshot.active` |
| Selected session | Selected fill + `RAIL_FOCUS` bar while rail focused | `rail_sel` + `Focus::Rail` |
| Rail focused | Focus bar and selected card; keyboard Up/Down/Enter | `Focus::Rail` |
| Terminal focused | Active card fill; rail selection loses its strong fill | `Focus::Terminal` |
| Running | `●` + `RAIL_OK` + `running` text | real shell phase |
| Prompt/input | `○` + `RAIL_DIM_INK` + `prompt`/`input` text | real shell phase |
| Exited/failed | `×` + `RAIL_BAD` + `exit N` | real shell state/exit |
| Hover | Defined but not implemented in this slice | future pointer tracking |
| Disabled | Not applicable: this slice adds no new controls | — |

Disabled states for future controls must be derived from the actual gate/state,
not from a decorative condition. A disabled control must never look actionable.

## 9. Narrow windows and collapse

- `Ctrl+Shift+M` toggles the rail. Collapse is instant; there is no animation
  and no synthetic transition.
- The terminal viewport is recomputed from the resolved layout and every PTY is
  resized to exactly the drawn grid; no PTY is left at a stale size.
- Below 16 rail rows the compact layout drops optional rows. The 40-column
  terminal minimum still causes the rail to be refused rather than shrinking
  the terminal below a usable width.
- Narrowing the terminal does not reflow existing wrapped rows (`zet-term`
  limitation). The UI must not claim lossless reflow or restored TUI content.
- At 1024 x 768 the rail, three tabs, cards and footer remain readable.

## 10. Keyboard, pointer and accessibility

- Existing bindings are preserved: `Ctrl+Shift+T/W`, `Ctrl+PgDn/PgUp`,
  `Ctrl+Shift+M`, `Ctrl+Shift+C/V/A`, `F12`, `Ctrl+G`.
- Clicking the rail focuses it and selects the session under the pointer; both
  rows of a card select the same session. Clicking the terminal returns focus
  and starts a selection.
- Rail keyboard: Up/Down move selection, Enter activates, Esc returns to the
  terminal. Typing never leaks to the PTY while the rail is focused.
- State is conveyed by text + glyph + color, not color alone.
- Text contrast targets WCAG AA 4.5:1; focus/marker contrast targets 3:1. The
  token test enforces the current values.
- There is no native accessibility tree/screen-reader surface in this baseline.
  This is a known limitation; the chrome remains textual and keyboard-reachable.

## 11. Performance, motion and reduced motion

- No animation, tween, timer or new thread is added. Frames remain demand-driven
  with the existing 250 ms idle heartbeat.
- The rail adds a small fixed number of quads per frame: one rule, one footer
  fill, one or two fills per selected/active card and text cells already
  covered by the row loop. No grid clone, no per-frame allocation of terminal
  content, no new dependency.
- `draw_terminal` still clears the quad buffer first; chrome is staged after the
  terminal and before overlays, preserving the existing z-order contract.
- Reduced motion is inherently satisfied: there is no motion in this slice.
  Future motion must honor `reduced-motion` and must not delay the first prompt.
- Release gates remain: startup < 300 ms, input < 3 ms, idle RSS < 100 MB,
  `cat 100k` < 0.9 s. This slice does not relax them.

## 12. Honesty contract

- Sessions are the real open tabs. Cwd, shell phase and exit codes come from the
  live PTY/shell-state path.
- Commands are real recorded blocks for the active tab. Unknown values remain
  `—`; no placeholder is rendered as a live fact.
- No mission, CI, agent, approval, checkpoint, verification, token, GPU or
  context telemetry is fabricated.
- The reference image's synthetic mission/CI content is not copied. The only
  data shown in screenshots is the real temporary shell used for verification.

**Known pre-existing limitation:** `tmux 3.8` works in the real PTY, but
`NVIM v0.12.6` reports `E1568: Terminal did not respond to DSR request for
'background' color` and renders incompletely on **both** this branch and
`master`. The cause is a missing DSR/OSC color-query response in `zet-term`,
not the chrome slice. It is recorded in
[`evidence/p2b1/COMPATIBILITY.md`](evidence/p2b1/COMPATIBILITY.md) and must be
fixed in a separate terminal-compatibility change before advertising Neovim
support. This slice does not claim Neovim compatibility.

## 13. Verification and acceptance

- Reference asset verified by dimensions, byte count and SHA-256 before commit.
- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Genuine native before/after captures in
  [`docs/design/evidence/p2b1/`](evidence/p2b1/), including 1920 x 1080 and
  1024 x 768 states.
- Independent architect/auditor review of the exact PR head before owner merge.
  The author does not sign off on their own change.

## 14. Follow-ups (not authorized by this slice)

1. Optional contextual Inspector with its own evidence/provenance contract.
2. Pointer hover for rail cards, with a bounded repaint rule.
3. A real lightweight status surface only after its data sources exist.
4. Theme/token configuration (A4's deferred theme system).
5. ASCII ZET welcome identity after separate owner artwork approval.
6. Mission/outcome surfaces only when real `Mission`/`WorkEvent` sources exist.
