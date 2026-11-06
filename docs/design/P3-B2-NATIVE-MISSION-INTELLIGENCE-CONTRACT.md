# P3-B2 — Native Mission Intelligence Contract

**Status:** Stage B implemented on `feat/p3b2-native-mission-intelligence`.
**Base:** `origin/master` at `3ba5c6e0da8b8b1c3f15097acff74fa392567698`
**Scope:** Native, interactive, evidence-linked mission experience. No AI, no cloud, no browser/webview.

## 1. Data identity guarantees

- Mission identity is the persisted SQLite `missions.id` (stable, never a row index).
- Command identity is the exact `(block_id, session)` pair from `zet_blocks::Store`.
- Links persist via `Store::link_command` / `unlink_command`; unlink never deletes the recorded block.
- Availability is resolved against `blocks` at read time; missing evidence is labeled unavailable.
- The UI clears stale command selections on session switch/close and never derives identity from row position or command text.

## 2. Implemented keyboard and mouse controls

- `Ctrl+Shift+M`: toggle the Mission Rail (unchanged).
- `Ctrl+Shift+N`: switch the rail between Sessions and Missions. Enabling Missions also opens the Inspector evidence surface.
- Mission rail: `Up`/`Down` move selection; `Enter` focuses the Inspector; `n` opens New Mission. Mouse click selects a mission; the `+ NEW MISSION [n]` row opens the editor.
- New Mission overlay: type a title, `Enter` persists, `Esc` or `Ctrl+C` cancels. While the overlay is active every key is consumed by the editor and never reaches the PTY.
- Inspector linking: select a recorded command in Session or Timeline, then press `m` (Inspector or Rail focus). `m` is conflict-free; `a` remains the Agents view toggle.
- Mission evidence: `Up`/`Down` select a linked command; `d` removes the link; `Esc` returns focus to the terminal. Mouse click selects a link row.

## 3. Architecture

- One `zet_blocks::Store` is owned by the UI thread (`MissionState`). `rusqlite::Connection` is not `Sync`; it is never shared across threads.
- Mission list and evidence reads are bounded (`list_missions_bounded`, `mission_commands_bounded`; hard caps of 64 rows).
- The rail and Inspector are event-driven: mission data is reloaded on explicit mutations and mode changes, never per frame.
- Evidence rows fetch only metadata for the bounded visible set; no full command output is loaded for summary rows.
- Writes update `MissionState` only after the Store reports success; failed writes keep the previous visible state and show the error.

## 4. Error and empty states

- Store unreadable → explicit `MISSIONS UNAVAILABLE` state, never an empty list.
- No missions → `NO MISSIONS` empty state with the New Mission affordance.
- Link of an unavailable block → persisted as `[unavailable]`, never claimed as confirmed evidence.
- Database write failure → failure feedback in the rail footer; no premature success claim.

## 5. Visual direction

- Reuses existing Aperture chrome: dark graphite surfaces, restrained cyan/violet accents, existing typography.
- Mission Rail and Inspector proportions are unchanged; the terminal remains dominant.
- No decorative telemetry, progress percentages, or verification badges.

## 6. Deferred to later slices

- Mission graphs, AI inference, CI verification, file attribution, approvals, autonomous operation.
- Mission deletion CLI/UI (API exists).
- Full print-boundary control-character sanitization for legacy recorded command text.
- Evidence-list scrolling beyond the bounded viewport and batched evidence joins.

## 7. Verification gates

```
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```
