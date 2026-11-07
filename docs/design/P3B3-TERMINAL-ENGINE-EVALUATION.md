# ZET — P3-B3-A1 Terminal Engine Feasibility Evaluation

**Research date:** 2026-11-07  
**Status:** RESEARCH COMPLETE / PROTOTYPE NOT AUTHORIZED  
**Decision:** Investigate a bounded, isolated `libghostty-vt` 1.0 adapter spike before further large-scale `zet-term` protocol work. Do **not** migrate, merge an engine change, or install on this research alone.  
**Source baseline:** ZET `master` following roadmap PR #64 (`2ed85f95`). P3-B3-R1 CWD repair PR #65 remains a separate draft and should not be bundled into an engine change.

## Executive finding

ZET's product differentiator is its native Aperture workflow, real shell sessions, observable command evidence, user-linked missions, and contextual Inspector—not inventing another VT parser. `libghostty-vt` reached 1.0 with a stable C ABI on November 6, 2026, and is a serious candidate to supply core terminal parsing, grid, scrollback, reflow, protocol responses, and input encoding. This could address several P3-B3 discovery gaps, but it **does not** supply a finished Rust/wgpu renderer, PTY management, or ZET's provenance-aware application UI. A migration would require substantial integration work and could regress features already shipped.

This document is an evidence-based **go/no-go plan for a spike**, not a compatibility claim or architecture approval.

## Grounded ZET architecture inventory

- `zet-term` is a custom Rust grid and escape-sequence parser with bounded scrollback, OSC 133 shell markers, alt screen, cursor, terminal modes, and resize. The discovery report identifies missing DA/CPR/DECRQM/kitty negotiation, incomplete keyboard support, TUI mouse forwarding, and no wrapped-line reflow.
- `zet-gpu` depends directly on `zet-term` and implements native rendering with `wgpu` 30, `winit` 0.30, and `cosmic-text` 0.19.
- `zet-pty` uses `portable-pty` 0.9. Existing PTY and CWD correctness improvements should be preserved independently.
- `crates/zet/src/window/mod.rs` holds a `Tab` with a `zet_term::Terminal`. PTY reader chunks flow into `Tab.term.feed(...)`, then recorders, scrollback search, command selection, renderer, clipboard, Inspector, and input handling consume terminal state. An engine change therefore impacts multiple interfaces, not just a Cargo dependency.
- Mission linking, recorded block persistence, and provenance live above the emulator. Their invariant is that output, exit status, session identity, cwd, timestamps and explicit user links remain truthful. Their implementation must not silently infer agent progress or completion.

## External evidence and licensing (as checked 2026-11-07)

1. Ghostty author Mitchell Hashimoto announced `libghostty-vt` **1.0** on Nov 6, 2026: stable C ABI, opaque handles, ABI checks, versioned ELF symbols, signed source/dependency distributions, MIT library license. <https://mitchellh.com/writing/libghostty-1-0>
2. Ghostty maintainer guidance explicitly says `libghostty-vt` exposes render state but **not** the application's renderer; embedders should write their own renderer. <https://github.com/ghostty-org/ghostty/discussions/12131>
3. Packaging documentation: `libghostty-vt` has a separate `vt-X.Y.Z` release cycle, needs a **version-specific Zig toolchain** (1.0.0 calls for Zig 0.17.x), offers signed source/dependency tarballs for offline builds, static/shared libs, headers and `pkg-config`. <https://github.com/ghostty-org/ghostty/blob/main/PACKAGING-LIBGHOSTTY.md>
4. A third-party Rust binding exists, but versioning is **not necessarily aligned with the stable 1.0 C ABI**. Examples include `ghostty-vt` pinned to a Ghostty source commit and `libghostty-vt` crate versions from earlier APIs. Audit maintenance, headers, build scripts and ABI before selecting any wrapper. <https://docs.rs/ghostty-vt/latest/ghostty_vt/> and <https://docs.rs/libghostty-vt/latest/libghostty_vt/>.
5. Reference user-facing competitive workflow: cmux uses Ghostty technology but its split panes, sidebar, agent notifications and workspace control are independent product functionality—not supplied automatically by the VT library. <https://cmux.com/>

## Scope and architectural boundary

| Area | Possible migration | Must remain ZET-owned |
| --- | --- | --- |
| VT sequences / screen / scrollback | `libghostty-vt` | Application session/identity and provenance |
| Cursor / modes / resize reflow | `libghostty-vt` | Accurate viewport-to-PTY geometry and layout |
| Keyboard / mouse encoders | Evaluate library input API | winit event capture, shortcuts, focus, selection policy |
| Graphics protocol state | Evaluate capabilities and memory limits | Native wgpu draw path, image upload, bounds and GPU resources |
| OSC 133 and command recording | Explicit adapter contract required | Recorder, block provenance, immutable association semantics |
| Events / query responses | Library events marshaled safely to PTY | Permission-sensitive host actions, clipboard policy, cancellation |
| PTY spawn / CWD / lifecycle | No change required | `zet-pty`, P3-B3-R1, shutdown/reap semantics |
| Tabs / splits / workspaces | No direct library support | `window/mod.rs`, Mission Rail, Issue #66 |
| AI CLI supervision | Library helps terminal compatibility only | ZET observation policy and optional model/agent integrations |

### Required security and correctness invariants

- Do not advertise Ghostty-specific terminal identity or `TERM` unless ZET implements the corresponding host-side interactions. Negotiate DA/DECRQM/kitty only when true. Reject unimplemented requests honestly.
- Gate OSC 52/1337/5522 clipboard writes by explicit ZET permission policy. Third-party binding defaults may allow them—do not inherit that behavior blindly.
- Treat PTY output as untrusted. Bound output buffering, image memory, search, and draw work; preserve demand-driven rendering and never let a misbehaving TUI freeze input.
- Preserve correct OSC 133 command boundaries, recorder sources, prompt/command/done transitions, selection/search semantics, scrollback behavior, timestamp/session IDs, undo, and inspector navigation.
- Keep terminal state single-thread-owned if the chosen FFI wrapper is `!Send/!Sync`; pass byte chunks and events over channels, not opaque handles.
- No writes to owner `~/.zet` or live sessions in tests. Use mechanically guarded isolated `ZET_HOME`; do not automate focus/keyboard into the active desktop.
- One-engine-at-a-time in the shipping build. Any prototype feature flag must default to existing `zet-term`, and must not silently change the canonical launcher.

## Bounded feasibility spike (separate owner authorization)

**Spike 0 — API and build proof (offline, isolated worktree):** Verify tagged `vt-1.0.0` artifacts, hashes/signatures, Zig compatibility, Linux x86_64 linkage, licensing, and a minimal Rust FFI wrapper. Determine whether a maintained wrapper truly targets the stable 1.0 C ABI. No install or normal user DB.

**Spike 1 — Headless parity harness:** Feed a shared corpus of VT streams through current `zet-term` and a test-only `libghostty-vt` instance. Cover: basic colors, UTF-8 and wide characters, alternate screen, scrollback, 80→40→80 reflow, CPR/DA/DECRQM replies, OSC 10/11, OSC 133 markers, bracketed paste, Kitty keyboard queries, mouse mode, large output. Record snapshots and event bytes; classify intentional differences. Do not count an unproven feature as fixed.

**Spike 2 — Renderer adapter proof:** Render **one test-only static or PTY-backed viewport** from Ghostty render-state into ZET's existing wgpu path. Preserve fonts, selection, cursor, resize behavior, screen coordinates, damage decisions, and theme. Do not touch Mission Rail or Inspector implementation until this succeeds. Synchronized-output mode must not deadlock drawing.

**Spike 3 — Provenance contract:** Demonstrate command input and exact output through OSC 133, into a **disposable** recorded block with session, cwd, git branch, exit and duration. No fabricated work. Compare to the existing ZET recorder on the same fixture.

**Spike 4 — Benchmark and decision:** Compare startup, warm input-to-pixel latency, terminal parsing and GPU frame time, `cat 100k`, idle CPU/RSS, resize, Wayland clipboard behavior, nvim, tmux, less, OpenCode idle TUI, and high-output stress against baseline. Use isolated GUI session or record GUI tests as pending. Report exact machine, source SHA, flags, timing method, memory, CI, and confidence; no invented speedups.

## Go/no-go criteria

**GO for a migration design only if:** stable version-pinned distribution and licensing are feasible; the custom GPU renderer can consume render-state without degrading Aperture; OSC 133 and the recorder remain truthful; keyboard, mouse, query and reflow defects demonstrably improve; no critical TUI regressions; performance gates stay at or improve upon validated ZET baseline (or there is a documented, owner-accepted tradeoff); dependency maintenance and build complexity are acceptable.

**NO-GO (retain `zet-term`) if:** adapter introduces excessive duplication/fragility; unsupported features cannot be safely disabled; shell evidence is lost; renderer cannot show correct selection/Unicode/scrollback/graphics; build burden or performance outweighs gains. A partial hybrid is not automatically safer: avoid running two divergent terminal states for one PTY without a precisely proved synchronization boundary.

**Decision authority:** independent technical audit of spike evidence → architect recommendation → explicit owner approval of any implementation migration, merge or install.

## Product sequencing

1. Finish R1 CWD owner GUI acceptance and merge/release decision on its own merits.
2. Authorize and complete a headless `libghostty-vt` integration spike; **do not** stall unrelated small terminal safety corrections.
3. Decide whether P3-B3-R2 keyboard/queries/mouse/reflow should use the current engine or a controlled `libghostty-vt` migration.
4. Independently advance Issue #66 first-slice workspace/session controls. Split panes and layout/process persistence are later, separately approved.
5. Re-test daily AI coding CLI workflows after terminal compatibility improves.

## Current verdict

**PROMISING / EXPERIMENT REQUIRED.** Research supports a bounded spike. It does not establish a shipping integration, working Rust bindings at 1.0, compatible GPU renderer, performance wins, or completed functionality in ZET.
