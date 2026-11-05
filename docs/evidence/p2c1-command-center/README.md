# P2-C1 — native before/after evidence

These are real native captures of the ZET application running as a Wayland
client in Hyprland 0.56.2. The host session was locked, so the captures were
taken from a nested Hyprland session with a 1920×1080 headless output. The
application, compositor, GPU rendering path, PTY, recorder, and fonts are real;
no mockups, synthetic terminals, or composited fake output were used.

Before captures use the approved baseline binary at
`a60507090ddf24517291f00249575252a1b5d790`. After captures use the P2-C1
feature head recorded in the PR/report. The command sequences are genuine
shell commands run in the captured session; exact timings and session ids
differ between before/after runs because each run is a fresh real session.

## Matching pairs

| Pair | Viewport | Before | After |
|---|---|---|---|
| Workspace / Mission Control | 1920×1080 | `01-before-workspace-1920.png` | `11-after-workspace-1920.png` |
| Rail focused + selected command | 1920×1080 | `02-before-rail-focus.png` | `12-after-rail-focus.png` |
| Inspector focused + selected command/output | 1920×1080 | `03-before-inspector-focus.png` | `13-after-inspector-focus.png` |
| Agents / candidate evidence | 1920×1080 | `04-before-agents.png` | `14-after-agents.png` |
| Empty selection | 1920×1080 | `05-before-empty.png` | `16-after-empty.png` |
| Terminal focused | 1920×1080 | `06-before-terminal.png` | `17-after-terminal.png` |
| Standard desktop | 1440×900 | `07-before-1440.png` | `18-after-1440.png` |
| Narrow layout | 1024×768 | `08-before-1024.png` | `19-after-1024.png` |
| OpenCode full-screen | 1920×1080 | `09-before-tui.png` | `20-after-tui.png` |

Additional after-only state:

- `10-after-welcome.png` — approved P2-B identity, unchanged by P2-C1.
- `15-after-timeline.png` — cross-session observed work timeline.

## Comparison sheets

- `21-compare-workspace.png` — before left, after right.
- `22-compare-inspector.png` — selected-command evidence hierarchy.
- `23-compare-agents.png` — grounded candidate evidence.
- `24-compare-narrow.png` — 1024×768 responsive behavior.
- `25-compare-1440.png` — 1440×900 composition.
- `26-compare-tui.png` — OpenCode compatibility.

## What the after state demonstrates

- Larger, consistent native type hierarchy without changing PTY geometry.
- Active session (cyan live marker) versus selected session (violet contextual
  seam) versus rail keyboard focus (cyan outline).
- Command indices, aligned command text, semantic exit/duration metadata, and
  failure markers.
- Inspector session band with real state chip, CWD, last exit, and observed
  count; no invented values.
- Session-scoped evidence index that uses its width for commands instead of
  repeating the active session id.
- Selected command card with observed result, execution context, bounded output
  well, and provenance footer.
- Truthful effective mode label (`MISSION CONTROL`, `WORKSPACE`,
  `EVIDENCE VIEW`, `TERMINAL FOCUS`).
- OpenCode full-screen remains unobstructed.
