# P3-B2 — Native Mission Intelligence Contract (Stage A)

**Status:** Stage A technical contract. Implementation of the interactive slice is tracked on branch `feat/p3b2-native-mission-intelligence`.
**Base:** `origin/master` at `3ba5c6e0da8b8b1c3f15097acff74fa392567698`
**Scope:** Native, interactive, evidence-linked mission experience. No AI, no cloud, no browser/webview.

## 1. Data identity guarantees

- Mission identity is the persisted SQLite `missions.id` (stable, never a row index).
- Command identity is the exact `(block_id, session)` pair from `zet_blocks::Store`.
- Links persist via `Store::link_command` / `unlink_command`; unlink never deletes the recorded block.
- Availability is resolved against `blocks` at read time; missing evidence is labeled unavailable.
- All UI state that references a command carries `(block_id, session)`; stale identity is cleared on session switch/close.

## 2. Keyboard and mouse controls (planned)

- `Ctrl+Shift+M`: toggle Mission Rail (existing).
- Mission list: `Up`/`Down` (or `j`/`k`) to move selection; `Enter` to select mission; `n` to start New Mission.
- New Mission overlay: type title; `Enter` creates; `Esc` cancels. Typed characters never reach the PTY while the overlay is active.
- Inspector Mission view: `a` adds the currently selected recorded command to the selected mission; `d` removes the selected link.
- Mouse: click mission row to select; click New Mission / Add / Remove buttons.

## 3. Architecture

- A single `zet_blocks::Store` is opened per application instance (default path) and shared read-only for mission lists; writes go through the same Store and invalidate the rail/inspector caches.
- Rail and Inspector consume immutable snapshots (`RailSnapshot`, `InspectorSnapshot`), refreshed only on store mutation or session change — never every frame.
- Mission list rendering is bounded (fixed cap, e.g. 64 rows) with scroll offset; no unbounded UI refresh.
- Evidence retrieval for a mission uses `mission_commands` + `mission_command_block` per visible row only (bounded by viewport), with cache invalidation after writes.
- No SQLite access on the render hot path beyond cached snapshots.

## 4. Error and empty states

- Store unreadable → explicit "MISSIONS UNAVAILABLE" state, never an empty list.
- No missions → "NO MISSIONS" empty state with New Mission affordance.
- Link of an unavailable block → persisted as `[unavailable]`, never claimed as confirmed evidence.
- Database write failure → failure feedback, no premature success claim.

## 5. Visual direction

- Reuse existing Aperture chrome: dark graphite surfaces, restrained cyan/violet accents, existing typography hierarchy.
- Mission Rail and Inspector proportions unchanged; terminal remains dominant.
- No decorative telemetry, progress percentages, or verification badges.

## 6. Deferred to later slices

- Mission graphs, AI inference, CI verification, file attribution, approvals, autonomous operation.
- Mission deletion CLI/UI (API exists).
- Full print-boundary control-character sanitization for legacy recorded command text.
- Bounded query caps and N+1 elimination in `mission_commands` (acceptable at personal scale for this slice).

## 0. Corrected architectural decisions (Stage B)

- Baseline corrected to master merge `3ba5c6e0da8b8b1c3f15097acff74fa392567698`.
- Keyboard: `a` remains Agents view toggle; Add to Mission uses a conflict-free binding (planned `m`) plus visible on-screen controls.
- SQLite ownership: `rusqlite::Connection` is not `Sync`; use a single owner/worker with request-response messages; never share across threads; never block render/PTY input.
- Rail geometry: Missions use an explicit rail mode / bounded section; no unbounded row insertion; hit-testing aligned to visible rows.
- Rendering safety: bound queries and cached snapshots; sanitize persisted metadata before render; no per-frame SQLite.

## 7. Verification gates

All four must pass before any change is considered done:

```
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```
