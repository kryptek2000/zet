# P3-A1 — Evidence-Linked Commands Contract

**Status:** Docs-only architecture contract. No P3 implementation is authorized by this document.  
**Tracking:** GitHub Issue #41 / next milestone after P2-B2  
**Base:** master at `91d409bd521a5127c2c14d22bd9ecc1513d2ba7b`

## 1. Product purpose

P3-A1 makes ZET's existing recorded shell commands inspectable in the native Contextual Inspector.

A developer runs real commands in ZET, opens the Inspector, and sees the recent commands recorded for the active session. Each row is grounded in an existing `Block` from `zet-blocks`. ZET does not invent file relationships, CI results, approval state, verification receipts or mission progress.

## 2. Existing source capability inventory

### Already implemented and usable

- `zet-term`: VT parsing and OSC 133 shell-integration markers.
- `zet-pty`: local PTY and resize.
- `zet-blocks::Block` persists:
  - `id`
  - `session`
  - `command`
  - `output`
  - `exit_code`
  - `cwd`
  - `git_branch`
  - `duration_ms`
- `zet-blocks::Recorder`:
  - `blocks()` loads all session blocks.
  - `block_count()` counts session blocks.
  - `recent_commands(limit)` loads recent metadata without output blobs.
  - `store()` exposes the underlying store.
- `zet-blocks::Store`:
  - `list(session)` returns all blocks for a session.
  - `recent_commands(session, limit)` returns bounded metadata.
  - `count(session)` counts blocks.
  - `append()` scrubs secrets before persistence.
- `zet-context::git_state(cwd)` provides branch and dirty count on demand.
- `zet/src/window/panels.rs`: `ChromeCache`, `InspectorCache`, formatting and truncation helpers.
- `zet/src/window/mod.rs`: `App`, `Tab`, focus handling, layout resolution, `shell_cwd`, and the Inspector viewport.

### Implemented but not connected as P3

- Inspector currently displays only session, state, exit and CWD.
- Recorded commands are not displayed in the Inspector.
- No command selection detail view exists.
- No command-to-file, CI, approval or verification links exist.

### Not implemented

- Command timestamps or start/end wall-clock times.
- Command-to-file attribution.
- CI or test-result linkage.
- Approval/checkpoint/verification state.
- Mission grouping.
- Timeline navigation.
- Damage-based rendering.

## 3. Actual persisted path

Real flow today:

1. Shell emits OSC 133 markers through `zet-integration` scripts.
2. `zet-term` parses markers into `Marker` values with snapshotted grid positions.
3. `Recorder::pump()` converts markers into a `Block`.
4. `Store::append()` scrubs secrets and writes to SQLite.
5. `Recorder::recent_commands()` provides bounded metadata for display.

Important limitations:

- `finish()` reconstructs output from the bounded terminal grid/scrollback, not a lossless streaming capture.
- `duration_ms` is measured from the Exec marker to the Done marker.
- `git_branch` is captured once when the block finishes.
- `cwd` is captured when the block finishes.
- There is no persisted timestamp beyond `duration_ms`.
- `Store::recent_commands()` omits `output`, which is correct for list display.
- Secret scrubbing is best-effort and happens only at the store write path.

## 4. Proposed first slice scope

P3-A1 displays recent recorded commands for the active session in the Contextual Inspector.

### In scope

- Active session command rows.
- Command text.
- Exit status when known.
- Duration when recorded.
- Clear failure distinction.
- Working directory and Git branch where already available.
- Empty and unavailable states.
- Bounded retrieval and cache refresh.
- Keyboard selection within the Inspector.
- Bounded view of a selected command's recorded output.
- No terminal input leakage.

### Explicitly deferred

- Command-to-file attribution.
- CI verification receipts.
- Approval or checkpoint dashboards.
- Agent telemetry.
- Mission success claims.
- Timeline navigation.
- New schema migration.
- New crate.

## 5. Provenance and truth-source requirements

- Commands must come from the active tab's real `Recorder`.
- Exit code and duration must come from the recorded `Block`.
- CWD and branch must come from the recorded block or live session context.
- Unknown values must remain `—`.
- Block IDs are scoped to their storage database (SQLite `AUTOINCREMENT`); never treat an ID alone as globally unique across different stores.
- Working-directory metadata requires secret-safe persistence and display handling.
- A successful exit code does not certify a fix, test or mission.
- Failed commands must be visibly distinguished.
- Secret-scrubbed command text must still be treated as sensitive and must not be logged.

## 6. Native visual and component anatomy

Follow the existing native renderer and the approved flagship direction.

Suggested minimal component tree:

- `InspectorHeader`
- `SessionContextSection`
- `ObservedActivitySection`
  - `CommandRow`
- `InspectorEmptyState`

Visual rules:

- Deep graphite surfaces.
- Restrained cyan/teal highlights.
- Clear heading hierarchy.
- Controlled spacing.
- No fake metrics, CI states, agent activity or verification claims.
- Terminal remains dominant.

Command rows should reuse `format_command_row` where possible. Long commands truncate with an ellipsis and never clip duration or exit code.

## 7. Layout and geometry contract

Use the existing `layout::resolve` resolver. Do not duplicate geometry.

- Inspector visibility is independent of rail visibility.
- Terminal viewport remains authoritative.
- Minimum terminal width remains `MIN_TERM_COLS`.
- Inspector width remains `INSPECTOR_COLS`.
- Narrow windows refuse the Inspector before reducing the terminal below its minimum.
- Inspector coordinates are window-relative and translated to the Inspector viewport.
- Inspector clicks never become terminal input or selections.

## 8. Performance and safety

The Inspector must not:

- block PTY reading
- stall the UI thread
- rebuild expensive data every frame
- cause continuous rendering while idle
- require a cloud service or AI model
- add a new timer or thread

### Retrieval strategy

- Use `Recorder::recent_commands(limit)` with a small fixed limit, such as 8.
- Refresh only when cheap state changes or the existing 250 ms poll interval elapses.
- Reuse the bounded `InspectorCache`.
- Do not load output blobs for list display.
- Do not call `Store::list()` for normal rendering.
- Do not call Git from the UI thread for display.

### Secret handling

- Command text is already scrubbed by `Store::append()`.
- Working-directory metadata is scrubbed by `Store::append()` as well; previously stored (legacy) unsanitized paths must be scrubbed again at display time.
- Treat scrubbed text as sensitive.
- Do not print command text to debug logs.
- Do not expose full output blobs in the Inspector list.

## 9. Error, empty and narrow states

- No recorder: render `No recorded commands`.
- No blocks: render `No recorded commands`.
- Unknown exit status: render `—`.
- Unknown duration: render `—`.
- Zero duration: render `0ms`, distinct from unknown.
- Narrow window: Inspector is refused and focus returns to the terminal.
- Degraded layout is reported truthfully.

## 10. Deterministic tests

Required tests:

- Active-session history comes from the active tab's recorder.
- Tab switching changes the displayed command history.
- Session replacement at the same tab index invalidates stale history.
- Failed and successful commands are distinguished.
- Missing exit or duration renders `—`.
- Zero duration renders `0ms`.
- Long commands truncate without clipping duration or exit code.
- Narrow windows refuse the Inspector.
- Empty history renders an empty state.
- Sanitization removes control characters and bounds text.
- Cache rebuild rules.
- No terminal grid mutation.

## 11. Independent audit criteria

The auditor should verify:

- Only real observed data is displayed.
- No fabricated state.
- No SQLite or Git work on the UI thread beyond the existing bounded refresh pattern.
- No terminal input leakage.
- No performance-gate regression.
- Scope remains limited to P3-A1.

## 12. Dependencies

- Depends on merged P2-B2 Inspector foundation.
- Depends on existing `zet-blocks` recorder and store.
- Does not depend on PR #35 visual polish.
- Does not depend on PR #39 instrumentation for correctness.
- Does not require schema migration.

## 13. Exact next implementation slice

1. Add command rows to `InspectorSnapshot`.
2. Extend `InspectorCache` invalidation for block count changes.
3. Build the snapshot from `recorder.recent_commands(8)` only.
4. Render command rows using existing formatting helpers.
5. Add keyboard selection and empty/unavailable states.
6. Add deterministic tests.
7. Open a draft PR from current master.

Do not implement command detail, file attribution, CI linkage, approvals or verification claims in this slice.

STOP. This is a docs-only contract. No implementation, build, launch, commit, push or merge is authorized by this document.
