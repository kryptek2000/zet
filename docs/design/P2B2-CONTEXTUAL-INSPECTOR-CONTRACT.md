# P2-B2 — Contextual Inspector Foundation Contract

**Status:** Docs-only implementation-ready contract. No Inspector code is implemented by this document.  
**Tracking:** GitHub Issue #41  
**Base:** master at `7ce2709d09a686638ed35144c9b37d1e2918d740`  
**Product direction:** ZET — The Outcome-Aware Terminal

## 1. Product purpose

P2-B2 adds a native, read-only Contextual Inspector as the right-hand panel of the approved flagship composition:

**Mission Rail | Living Terminal Workstream | Contextual Inspector**

The Inspector reveals context about actual developer activity. It is not an IDE, AI chat sidebar, agent dashboard, generic terminal clone, or replacement for the terminal. The real PTY remains the source of truth for shell interaction.

The first slice is intentionally limited to context that ZET already observes or records:

- active session identity
- actual working directory
- actual Git branch when available
- real shell/session state
- recently recorded commands
- exit status when known
- duration when recorded
- clearly distinguished failed commands

Unknown information must remain unknown. No placeholder may be rendered as a live fact.

## 2. Source capability inventory

### Already implemented and usable

- `zet-term`: VT grid, scrollback, alt-screen, bracketed paste, shell integration, resize without reflow.
- `zet-pty`: local fork/exec PTY and resize.
- `zet-blocks`: SQLite command block store/recorder and secret scrubbing. Blocks contain session, command, output, exit code, cwd, git branch and duration.
- `zet-context`: `git_state(cwd)` provides branch and dirty count; context gathering and scrubbing.
- `zet-gpu`: wgpu renderer, cosmic-text glyph atlas, tab bar, ghost/search/selection, quads and text rendering primitives.
- `zet/src/window/layout.rs`: pure deterministic layout resolver with `rail`, `inspector` and `term` viewports. Named modes include `TerminalFocus`, `Workspace` and `MissionControl`.
- `zet/src/window/panels.rs`: rail snapshot, formatting, hit-testing and chrome cache patterns.
- `zet/src/window/mod.rs`: `App.layout` is a `Layout`, `frame_layout()` resolves viewports, rail toggle exists, tab state is real.

### Implemented but not connected as Inspector UI

- `Layout::inspector` and `LayoutMode::MissionControl` exist in the pure layout resolver.
- `FrameLayout.inspector` viewport is calculated.
- No live Inspector panel rendering or hit testing is wired into `App`.
- No Inspector snapshot, focus state, scroll state or selection state exists yet.

### Proposed or unavailable

- Mission, CI, agent, approval, checkpoint and verification surfaces are not implemented as live Inspector data.
- No screen-reader/accessibility tree exists.
- No damage-based rendering is wired.
- No terminal reflow on resize.
- No SSH transport or Windows ConPTY in this baseline.

Historical A5 examples are design references, not evidence of implementation.

## 3. First-slice scope

The first Inspector slice is a read-only contextual panel on the right side of the window. It displays only real observed session and command data.

### In scope

- Show/hide Inspector independently.
- Render the active tab's session context.
- Render recent recorded commands for the active tab.
- Show command exit status and duration when recorded.
- Visually distinguish failed commands.
- Support keyboard focus and pointer selection within Inspector rows.
- Preserve terminal input and selection when Inspector is focused.
- Handle resize, tab changes and narrow windows.
- Empty state when no data is available.
- Bounded snapshot/cache refresh, analogous to `ChromeCache`.

### Explicitly deferred

- Inspector actions, approvals, checkpoints, verification receipts.
- Agent telemetry, mission progress, CI status or file-change claims.
- Search inside the Inspector.
- Drag/resize of the panel.
- Persistent layout preferences.
- Accessibility tree.
- Any data source not already available from real PTY/session/block state.

## 4. Provenance and truth-source requirements

- Session identity, cwd, shell phase and exit codes must come from the live PTY/shell-state path.
- Commands must come from the active tab's real `Recorder`.
- Unknown values must render as `—` or an equivalent explicit unknown marker.
- No placeholder may be rendered as a live fact.
- Provenance must be distinguishable as observed, inferred or unknown where the source supports it.
- A command completing with exit 0 does not certify a mission or fix.
- UI state changes must never be presented as verified outcomes.

## 5. Native visual and component anatomy

The Inspector must use the existing native wgpu renderer and composition layer. It must feel like part of ZET, not an embedded website.

### Visual direction

- Dark graphite/deep-space surfaces.
- Restrained cyan/teal highlights.
- Clear type hierarchy.
- Subtle panel separation.
- Minimal visual noise.
- Terminal remains dominant.

### First-slice anatomy

Suggested minimal component tree:

- `InspectorHeader`
  - title: `Inspector`
  - collapse affordance
- `SessionContextSection`
  - session id or tab identity
  - cwd
  - git branch when available
  - shell phase
  - exit status when known
- `ObservedActivitySection`
  - recent command rows
  - exit status
  - duration when recorded
  - failure distinction
- `InspectorFooter` (optional, only if it shows real bounded status)

### Empty state

When no data is available, render an explicit empty state such as `No recorded commands`. Do not render synthetic example data.

### Selection

- Clicking an Inspector row selects it.
- Keyboard Up/Down moves selection.
- Enter may expand or focus detail only when a real detail view exists.
- Esc returns focus to the terminal.
- Typing never leaks to the PTY while the Inspector is focused.

### Narrow-window behavior

- The Inspector must collapse before the terminal drops below `MIN_TERM_COLS`.
- If both requested panels cannot fit, terminal width wins.
- Degraded layout must be reported truthfully through the existing `Degraded` model.
- No horizontal overflow or clipping of essential text.

### Scrolling and overflow

- If content exceeds viewport height, use bounded scroll state owned by the Inspector.
- Preserve terminal scrollback independently.
- Tab changes reset or restore selection according to documented rules.

## 6. Layout and geometry contract

Use the existing pure `layout::resolve` resolver. Do not duplicate geometry logic.

Required behavior:

- Inspector visibility is independent of rail visibility.
- `MissionControl` is the named preset with rail and inspector visible.
- `TerminalFocus` has neither rail nor inspector.
- `Workspace` has rail only.
- Terminal viewport is calculated from resolved `FrameLayout.term`.
- Inspector viewport is calculated from resolved `FrameLayout.inspector`.
- Minimum terminal width remains `MIN_TERM_COLS`.
- Inspector width remains `INSPECTOR_COLS` unless a future audited change modifies the layout contract.
- Resize recomputes all viewports from current window dimensions.
- Mouse coordinates are translated relative to the Inspector viewport, never the terminal viewport.
- Inspector clicks must not become terminal input or selections.
- Terminal Focus remains usable and fast independent of the Inspector.
- No lossless terminal reflow is promised; `zet-term` resize does not reflow existing wrapped rows.

### Focus rules

- `Focus::Terminal` and `Focus::Rail` already exist.
- Add `Focus::Inspector` only if needed, or reuse a generic panel focus model.
- Focus changes must not restart a terminal, lose scrollback or hide pending approvals.
- Typing while Inspector is focused must not reach the PTY.

### Tab changes

- Inspector content follows the active tab.
- Selection should reset to the first row or restore a matching command id if command identity is available.
- No stale command from a previous tab may remain displayed as active.

## 7. Performance and safety

The Inspector must not:

- block PTY reading
- stall the UI thread with database queries
- rebuild expensive data unnecessarily
- cause continuous rendering while idle
- require a cloud service or AI model
- introduce background agents or autonomous operations

### Snapshot strategy

Use a bounded cache analogous to `ChromeCache`:

- dirty flag
- last active tab
- last block count
- last cwd
- last refresh timestamp
- poll interval, default 250 ms
- rebuild only when observable state changes or the poll interval elapses

Data sources:

- active tab session state
- shell cwd and phase
- `git_state(cwd)` for branch and dirty count
- active tab recorder block count and recent commands
- existing block store only through already available recorder APIs

Do not query SQLite directly from the render path. Do not add a new dependency.

Preserve all published performance gates:

- cold startup < 300 ms
- input latency < 3 ms
- idle RSS < 100 MB
- `cat 100k lines` < 0.9 s

No motion, timer or new thread is added in the first slice.

## 8. Dependency strategy

PR #35 remains unmerged and under performance hold. PR #39 remains unmerged.

Implementation order:

1. Implement Inspector foundation on current `master`, independent of PR #35.
   - Pure layout wiring for `inspector` viewport.
   - Read-only Inspector snapshot from real session/block data.
   - Keyboard and pointer selection.
   - Empty, populated and narrow states.
2. Keep visual tokens minimal and neutral until P2-B1 styling is accepted.
   - Do not depend on P2-B1 rail colors or tab chrome.
   - Use existing renderer primitives only.
3. After PR #35 is merged and accepted, apply P2-B1 visual tokens and spacing to the Inspector.
   - This avoids overlapping renderer and layout changes.
4. Keep performance instrumentation separate.
   - Do not mix startup instrumentation with Inspector UI changes.
5. Block Inspector merge until:
   - PR #35 performance acceptance is resolved or explicitly waived by the owner
   - independent audit passes
   - owner visual approval is granted

Do not rebase, merge or modify PR #35 or PR #39.

## 9. Deterministic test requirements

Tests must not require a compositor or ZET GUI launch.

Required unit tests:

- Layout resolver already covers inspector viewports and degradation.
- Inspector row mapping and hit testing.
- Truncation and width budgeting.
- Empty-state snapshot.
- Selection movement and reset on tab change.
- Provenance of displayed fields.
- Cache rebuild rules.
- No terminal grid mutation.

Required integration tests where feasible:

- Real PTY session state appears in the snapshot.
- Recorded blocks appear as observed commands.
- Unknown values remain unknown.
- Narrow widths degrade truthfully.

## 10. Independent audit criteria

The auditor should verify:

- Only real observed data is displayed.
- No fabricated mission, CI, agent, approval, checkpoint or verification state.
- No terminal input is consumed by Inspector interactions.
- No PTY or renderer behavior regresses.
- Layout geometry matches the resolver.
- Focus and selection behavior are deterministic.
- Performance gates are preserved.
- Scope remains limited to the first Inspector slice.

## 11. Exact next implementation slice

The first implementation slice should be:

1. Add `Focus::Inspector` and Inspector snapshot state to `App`.
2. Wire `frame_layout().inspector` into rendering using existing quads and text primitives.
3. Build a bounded `InspectorCache` using real active-tab session and recorder data.
4. Implement keyboard navigation and pointer hit testing.
5. Implement empty, populated, degraded and narrow states.
6. Add deterministic tests.
7. Open a draft PR on a dedicated branch from current master.

Do not implement Inspector actions, agent data, CI data, approvals, checkpoints or verification claims in this slice.

STOP. This is a docs-only contract. No implementation, build, launch, commit, push or merge is authorized by this document.
