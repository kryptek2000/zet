# P3-B1 — Mission Intelligence Foundation Contract

**Status:** Implementation contract and record for the P3-B1 bounded slice.
**Base:** master at `609da784bb20371f744c4e6427c90cdde11c288d`
**Branch:** `feat/p3b1-mission-intelligence`

## 1. Source capabilities used

- `zet-blocks::Store` SQLite persistence for recorded commands (`blocks` table).
- `Block` identity: global SQLite `id` plus `session` string.
- CLI dispatch in `crates/zet/src/main.rs`.
- Existing `env_session()` for session identity.

## 2. Mission data model

- `Mission { id, title, description, created_at, updated_at }`
- `MissionCommand { mission_id, block_id, session, provenance, created_at, available }`
- Stable command identity is `(block_id, session)`, not row position.
- Availability is resolved against `blocks` at read time; missing evidence is reported, not reassigned.

## 3. Persistence and migration

- Additive tables in the existing `blocks.sqlite`:
  - `missions`
  - `mission_commands` (composite PK `(mission_id, block_id, session)`)
- Created with `CREATE TABLE IF NOT EXISTS` on every open, including legacy databases.
- No existing recorded command data is modified.

## 4. Provenance

- All associations are explicit user actions (`provenance = "user"` by default).
- No automatic inference, no fabricated verification state.
- Mission completion is not inferred from command exit codes.

## 5. Implemented interactions (CLI vertical slice)

- `zet mission create <title> [--description TEXT]`
- `zet mission list`
- `zet mission show <id>`
- `zet mission link <mission_id> <block_id> [--session S]`
- `zet mission unlink <mission_id> <block_id> [--session S]`

Linking is idempotent; unlinking does not delete the underlying command.

## 6. Native UI

Native panel integration is deferred to a follow-up slice. The data model and CLI provide the evidence-connected foundation and are independently testable.

## 7. Performance

- No per-frame database queries added to the UI; mission data is only read on explicit CLI invocation.
- Terminal rendering and PTY paths are untouched.

## 8. Tests

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Added `crates/zet-blocks/tests/missions.rs` (8 tests).
EOF
echo done