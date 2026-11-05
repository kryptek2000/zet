# P2-C1 — Command Center Refinement

**Status:** implementation slice for owner visual acceptance
**Scope:** native chrome, Mission Rail, Living Terminal framing, Contextual Inspector
**Authority:** P2-C1 brief; P2-B identity remains unchanged

## Intent

Bring the rest of ZET to the finish level of the approved signature identity.
The interface should read as one Aperture instrument: indexed navigation,
dominant living terminal, and an evidence inspector with explicit hierarchy.
The work is presentation-only. It does not add telemetry, orchestration,
approval semantics, PTY behavior, or new data sources.

## Typography scale

`zet_gpu::presentation` remains the single native type system. P2-C1 raises the
chrome scale while keeping terminal cells authoritative:

| Role | Size | Use |
|---|---|---|
| `UI_BRAND` | 28 | ZET identity deck |
| `UI_HEADER` | 16.5 | tab identity, panel titles |
| `UI_SECTION` | 15 | indexed section headings |
| `UI_SESSION` | 16.5 | session names |
| `UI_COMMAND` | 17 | selected command / focal evidence |
| `UI_VALUE` | 16 | observed exit, duration, state |
| `UI_BODY` | 15.5 | commands, paths, body copy |
| `UI_OUTPUT` | 14.5 | recorded output well |
| `UI_META` | 12.5 | labels, counts, provenance |

## Mission Rail

- Indexed `01 WORKSPACE`, `02 SESSIONS`, `03 COMMAND LEDGER`, `04 RESOURCES`
  headings use cyan indices, near-white titles, and a quiet datum line.
- Session cards separate index, name, state, and path. Active sessions keep the
  cyan live dot and seam; a selected session uses the violet contextual seam
  and border while the rail itself retains the cyan keyboard-focus outline.
- Command cards add a stable two-digit index, aligned command text, semantic
  exit/duration fields, and a right-aligned duration. Failure rows keep the red
  marker and red exit label.
- Resources report only `NO SOURCES` when no source exists; narrow rails omit
  the label rather than colliding with the section rule.
- Empty selection is explicit in the Inspector; no placeholder evidence is
  fabricated.

## Contextual Inspector

- Header keeps `CONTEXTUAL INSPECTOR` / `AGENT ACTIVITY` / `OBSERVED WORK
  TIMELINE`, with a real segmented view control and a cyan active seam.
- The session band presents `SESSION 01`, a real state chip, CWD, last exit,
  and observed-command count. Unknown values remain `—`.
- Evidence index rows keep the stable selection/hit-test contract; Session
  view spends its width on the command instead of repeating the active session
  id.
- Selected command card keeps the P3-A1/P4-A2/P4-A3 contracts: observed result,
  execution context, bounded recorded output, and provenance footer.
- Agents view states `CANDIDATE EVIDENCE, NOT VERIFIED STATE` and never upgrades
  a launch candidate into a verified execution.

## Terminal framing

- The Living Terminal remains the largest surface and the only PTY grid.
- Active tab gets a cyan datum under its segment; inactive tabs remain quiet.
- The mode chip is derived from the **effective** layout:
  `MISSION CONTROL` (rail + Inspector), `WORKSPACE` (rail only),
  `EVIDENCE VIEW` (Inspector only), `TERMINAL FOCUS` (neither).
- Focus colors are preserved: cyan for rail/Inspector keyboard focus, violet
  for the terminal boundary.

## Responsive behavior

- 1920×1080: full three-column Mission Control.
- 1440×900: full three-column Mission Control with compact command budgets.
- 1024×768: Inspector refused by the existing layout resolver; chrome reports
  `WORKSPACE` truthfully, rail and terminal remain usable.
- Narrow windows: Inspector/rail collapse before the terminal falls below the
  existing minimum width.
- The wordmark and welcome dismissal behavior from P2-B are untouched.

## Evidence

Native captures live in `docs/evidence/p2c1-command-center/`. They include
before/after pairs for the workspace, rail focus, Inspector selection, Agents,
narrow layout, 1440×900, and OpenCode full-screen. Captures were taken from the
real native app in a nested Hyprland Wayland compositor with a headless
1920×1080 output because the host session was locked; no mockups or synthetic
terminals were used.
