# A5 — Native Mission Control: Architecture & Implementation Plan

**Tracking:** UX-001 · **Phase:** P1-A (architecture) → defines **P2-A** (implementation)
**Status:** proposed — awaiting architect (GPT-6) review and independent audit (Beam)
**Author:** MiniMax (implementer) · **Date:** 2026-10-22
**Supersedes:** nothing. **Amends:** `docs/design/A4-ui-component-spec.md` (see §2.4).

**Scope of this document:** architecture and implementation planning only. No UI is built here.
The proposed architecture is additive to the existing native Rust + wgpu application. There is
no Electron, no webview, no browser, no JavaScript in the hot path.

**Authority chain:** product intent is `docs/ZET-PRODUCT-CHARTER.md` + `docs/ROADMAP.md`
(PR #14). Implementation constraints are `AGENTS.md`, `docs/ZET-DESIGN.md` §2/§7/§8, and the
code. Where the two conflict, the code and the engineering gates win (charter §"Non-negotiables").

---

## 1. Baseline — actual repository state at authoring time

Verified against the remote on 2026-10-22 (`git fetch --all --prune`, `gh pr list`, `gh run list`).

### 1.1 Merge status of the two gate PRs — neither is merged

| PR | Branch | State | CI | Notes |
|---|---|---|---|---|
| #13 `fix(zet-gpu): correct vertical ascent and horizontal sidebearing in text blit` | `fix/ci-text-blit-ascent` | **OPEN** (not merged) | ✅ green (run `39717777114`) | Mergeable. Fixes the only red test on `master`. |
| #14 `docs: establish ZET outcome-aware terminal vision and execution roadmap` | `docs/living-terminal-direction` | **OPEN** (not merged) | ❌ red (run `39770370915`) | Docs-only, but branched before #13, so it still carries the #13 bug. |

`master` is at `e9703f2` and is **currently red**: `crates/zet-gpu/tests/text.rs:54`
(`blit_preserves_caps_and_descenders`) fails with `assertion left == right failed: cap pixels
clipped` (CI run `39704109045`). PR #13 fixes exactly this and is green.

> **Blocker B1 (release state, needs the owner).** #13 is green but unmerged; #14 is red because
> it predates #13's fix. #14 must be rebased onto #13 (or master after #13 merges) before it can
> go green. **This plan does not merge, rebase, push to `master`, or change any release state.**
> P2-A cannot start against a red baseline; §9 sequences the dependency.

### 1.2 Working baseline used for this document

This branch (`docs/p1a-mission-control-architecture`) is based on `origin/fix/ci-text-blit-ascent`
(PR #13's head) so that CI is green while the audit runs. The only new content is this file.
`cargo check --workspace --all-targets` passes locally on this baseline.

### 1.3 Codebase facts (the ones the plan depends on)

| Fact | Source |
|---|---|
| 12 crates; `zet` is the composition root, `zet-gpu` is a pure renderer | `crates/*/Cargo.toml` |
| No new dependencies are required for P2-A | §5 |
| The window renders **one** grid at origin (0,0); the tab bar steals row 0 when >1 tab | `crates/zet/src/window/mod.rs:832-838` |
| `Renderer::draw_terminal` clears the quad buffer — it must be called first | `crates/zet-gpu/src/renderer.rs:487` |
| All quads are cell-quantised with **absolute** cell coordinates | `crates/zet-gpu/src/quad.rs:64-103` |
| `Terminal::resize` **truncates**; it does not reflow, and scrollback is untouched | `crates/zet-term/src/lib.rs:729-757` |
| `Cell` carries no soft-wrap flag | `crates/zet-term/src/lib.rs:12-20` |
| The block recorder keys off **absolute rows** snapshotted at parse time | `crates/zet-blocks/src/recorder.rs:106-228` |
| The glyph atlas is 16×16 = 256 slots and **resets wholesale** when full | `crates/zet-gpu/src/renderer.rs:427-441` |
| Idle redraw heartbeat is 250 ms; frames are demand-driven | `crates/zet/src/window/mod.rs:277,1315-1331` |
| `Ctrl+<letter>` is translated to a control byte (0x01–0x1a) and sent to the PTY | `crates/zet-gpu/src/input.rs:127-162` |
| Release perf gates: startup <300 ms, input <3 ms, idle RSS <100 MB, `cat 100k` <0.9 s | `AGENTS.md:34-45`, `docs/ZET-DESIGN.md:112-135` |

---

## 2. Capability inventory — implemented vs. planned

Honest baseline, per roadmap P0 ("no greenwashing README"). Everything in the *Planned* column is
**not** in the repository today.

### 2.1 Implemented and verified in the tree

| Area | What exists | Evidence |
|---|---|---|
| PTY | Real fork/exec PTY, real shell (`$SHELL -i`), resize, shutdown, SSH transport | `crates/zet-pty/src/lib.rs`, `shell.rs`; `crates/zet-pty/tests/{session,stress,shell_state}.rs` |
| Terminal | VT parser, grid, scrollback, alt screen, bracketed paste, OSC 133/633 markers, search, `reveal_row` | `crates/zet-term/src/lib.rs`; `tests/{terminal,vttest,stress}.rs` |
| Shell integration | bash + zsh hooks emitting OSC 133 `B`/`C`/`D`/`X`; `ZET_SESSION` exported | `shell/zet-integration.{bash,zsh}`; `crates/zet/src/window/mod.rs:52-58` |
| Renderer | wgpu surface, glyph atlas (cosmic-text), tinted quads, tab bar, ghost overlay, search highlight, selection, cursor | `crates/zet-gpu/src/renderer.rs` |
| Input | Chord predicates + PTY byte translation, tested without a window | `crates/zet-gpu/src/input.rs`; `tests/input.rs` |
| Blocks | SQLite store of real recorded commands (command, output, exit code, cwd, branch, duration), secret scrubbing, completion history | `crates/zet-blocks/src/lib.rs`, `recorder.rs` |
| Tabs | New/close/cycle, per-tab PTY + grid + recorder, click-to-switch tab bar | `crates/zet/src/window/mod.rs:874-915` |
| Agent | Planner/executor/verifier, T0–T3 policy, checkpoints, journal, one-key undo (F12) | `crates/zet-agent/src/lib.rs`, `crates/zet-policy/src/lib.rs` |
| Context | Repo map, git state, shell history, env | `crates/zet-context/src/lib.rs` |
| Models | Gateway trait (OpenAI-compat / Anthropic / local), BYOK keychain, metering | `crates/zet-models/src/*` |
| Perf harness | `ZET_PERF=1` stage marks, first-frame ms, input-latency µs, `cat 100k` settle probe | `crates/zet/src/window/mod.rs:1218-1234`; `crates/zet-gpu/src/lib.rs:perf_mark` |
| CI | Exactly the four gates (fmt, check, clippy `-D warnings`, test) | `.github/workflows/ci.yml` |

### 2.2 Planned only — NOT implemented (explicitly)

| Thing | Status | Where it belongs |
|---|---|---|
| Mission rail / inspector / any multi-pane layout | **Absent.** One full-width grid only. | P2-A (this plan) |
| Any `Mission` type, `WorkEvent`, `ArtifactRef`, `EvidenceLink`, `VerificationReceipt` | **Absent.** No such types exist. | Charter P1/P3 — *not* P2-A (§6.4) |
| Command palette, splits, theme system, 120 Hz tween framework | **Absent.** A4 specified them; the charter re-scoped them out of the flagship path. | Deferred — §8 |
| Verification receipts / test-CI linking | **Absent.** ZET cannot verify anything today. | P5 |
| Agent progress telemetry, mission health, completion % | **Absent and must stay absent.** | Never (§6.4) |
| Screenshot / framebuffer readback | **Absent** — no `copy_texture_to_buffer` anywhere in the tree. | P2-A slice 6 (needed for the roadmap's screenshot exit criterion) |
| Keybinding remapping config | **Absent** (`docs/ZET-DESIGN.md:197` promises it) | P6 |

### 2.3 Known gaps in the current implementation that P2-A inherits

| Gap | Evidence | Impact on Mission Control |
|---|---|---|
| `Terminal::resize` truncates instead of reflowing | `crates/zet-term/src/lib.rs:729-757` | Narrowing the terminal for a rail truncates the right edge of live rows. §7.3 + Blocker B2. |
| `Cell` has no soft-wrap flag | `crates/zet-term/src/lib.rs:12-20` | True xterm-style reflow is impossible today without adding one. |
| Recorder keys on absolute rows | `crates/zet-blocks/src/recorder.rs:176,220-228` | Any change that inserts scrollback rows mid-session desyncs recorded blocks. Reflow is therefore a **separate** change. |
| Glyph atlas resets wholesale at 256 slots | `crates/zet-gpu/src/renderer.rs:427-441` | Panel chrome must use a tiny, fixed glyph vocabulary (§7.4). |
| `Alt+<key>` drops the Alt modifier | `crates/zet-gpu/src/input.rs:127-162` | `Alt`-based shortcuts are unreliable → P2-A uses the `Ctrl+Shift+` family only. |
| No framebuffer readback | — | No visual regression harness today. |

### 2.4 Relationship to A4 (`docs/design/A4-ui-component-spec.md`)

A4 is a **historical** design for a Ghostty×Warp synthesis (splits, command palette, theme system,
animation framework). The charter re-scoped the flagship. A4's *load-bearing* findings are kept;
its *product* proposals are deferred:

| A4 element | Decision for P2-A |
|---|---|
| "Chrome lives in `crates/zet/src/window/`; `zet-gpu` stays a pure renderer" | **Kept.** Binding constraint. |
| "`draw_terminal` clears the quad buffer → staging order is terminal-first, overlays-after" | **Kept.** Hard contract (§5.2). |
| "No new crates" | **Kept.** P2-A adds modules inside `crates/zet/src/window/`, no crate. |
| `draw_terminal_in(origin)` generalization | **Kept** — but for a *panel* viewport, not splits. §5.2. |
| `draw_fill` / `draw_text_row` | **Kept** — exactly the primitives the rail/inspector need. |
| `SplitTree` (splits) | **Deferred** — the charter's flagship is rail + workstream + inspector, not splits. |
| `CommandPalette` | **Deferred** — P2-A's mode switch is a keybinding, not a fuzzy palette. |
| `ThemeDef`/`Palette`/`Tokens` | **Deferred to P2-B**, but P2-A must not hard-code more colours than today (it reuses the existing `const Rgba` values verbatim). |
| `Tween`/120 Hz animation | **Deferred.** P2-A has no animation: panel show/hide is an instant layout change. |
| `BlockView` (grid-anchored block headers) | **Deferred to P3.** P2-A shows recorded blocks in the *inspector*, not as grid overlays. |

---

## 3. The architectural problem, stated exactly

The current window is a **single-viewport terminal**:

```
window pixels ──► cols × rows (cells)          crates/zet/src/window/mod.rs:1296-1298
                       │
                       ├── row 0            tab bar (only when tabs > 1)   :832-834
                       ├── rows 1..rows     terminal grid                 :836-838
                       └── PTY size = (cols, rows - bar_rows)              :841-849
```

Mission Control needs **three simultaneous, independently collapsible regions**:

```
┌────────────┬──────────────────────────────────┬───────────────┐
│ mission    │  living workstream                │  inspector    │
│ rail       │  (the REAL PTY + real grid)       │  (optional)   │
│ (optional) │                                  │               │
└────────────┴──────────────────────────────────┴───────────────┘
```

Two facts make this tractable and two make it dangerous.

**Makes it tractable**

1. The renderer is a pure function `Snapshot → [Vertex]`; every quad is positioned by **absolute
   cell index** (`crates/zet-gpu/src/quad.rs:64-103`). A panel at columns `[0,24)` is already
   expressible — it needs a draw function with an x origin, not a new rendering model.
2. The grid is **cell-quantised** (one monospace cell grid for the whole window). Panels therefore
   share the terminal's exact cell metrics, so chrome and terminal text align pixel-perfectly
   with no font pipeline, no second atlas, and no new shaping dependency.

**Makes it dangerous**

1. **The PTY width must equal the terminal viewport width.** If the PTY stays `cols` wide while we
   only draw `cols - 56` columns, the shell wraps at the wrong width and the transcript desyncs
   from the screen. This is the single most important correctness constraint in P2-A.
2. **Narrowing the viewport truncates live rows** (`zet-term::Terminal::resize`, §2.3). Layout
   switching therefore has a visible, honest cost that must be designed for, not discovered in a
   demo.

---

## 4. Proposed architecture

### 4.1 The viewport model

Introduce **one** new concept in the composition root: a `Layout` that resolves the window's
columns into named viewports.

```rust
// crates/zet/src/window/layout.rs   (NEW module, pure — no GPU, no I/O)

/// Which chrome regions are visible. Both flags are independent; the
/// named layouts are just presets over the same two flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub rail: bool,
    pub inspector: bool,
}

/// The three named presentations from the Product Charter. They are
/// presets, not separate runtimes: one live app, one PTY per tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    TerminalFocus,   // rail off, inspector off  — today's ZET, byte for byte
    Workspace,       // rail on,  inspector off
    MissionControl,  // rail on,  inspector on
}
impl LayoutMode {
    pub fn layout(self) -> Layout;
    pub fn next(self) -> Self;
}

/// A cell rectangle inside the window grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub x: u32, pub y: u32, pub cols: u32, pub rows: u32,
}

/// Resolved geometry for one frame.
pub struct FrameLayout {
    pub mode: LayoutMode,
    pub rail: Option<Viewport>,      // None = collapsed
    pub term: Viewport,               // ALWAYS present
    pub inspector: Option<Viewport>,  // None = collapsed
    /// True when a requested panel could not be shown for lack of width.
    pub degraded: Option<Degraded>,
}

pub enum Degraded {
    /// The inspector was refused: the terminal would have been too narrow.
    InspectorRefused,
    /// Both panels were refused.
    BothRefused,
}
```

**Resolution rules (pure, deterministic, unit-tested):**

```
RAIL_COLS       = 24     // default width of the mission rail
INSPECTOR_COLS  = 32     // default width of the inspector
MIN_TERM_COLS   = 40     // the terminal never gets narrower than this
MIN_TERM_ROWS   = 10     // matches the existing row clamp (mod.rs:1298)

resolve(cols, rows, requested: Layout) -> FrameLayout:
    1. rail_cols      = if requested.rail    { RAIL_COLS } else { 0 }
    2. inspector_cols = if requested.inspector { INSPECTOR_COLS } else { 0 }
    3. if cols - rail_cols - inspector_cols < MIN_TERM_COLS:
           inspector_cols = 0 ; degraded = InspectorRefused
       if cols - rail_cols < MIN_TERM_COLS:
           rail_cols = 0 ; degraded = BothRefused
    4. term.cols = cols - rail_cols - inspector_cols
       term.x    = rail_cols
       inspector.x = rail_cols + term.cols
       term.y = rows_used_by_top_chrome   (0 or 1, see §4.4)
       rail.y    = term.y ; inspector.y = term.y
       all panels share term.rows
```

**Height is deliberately untouched.** Mission Control steals **columns only**. The terminal's row
count is exactly what it is today (`rows - bar_rows`). This preserves:

- the existing `bar_rows()` / `grid_rows()` semantics and every existing perf baseline;
- the scrollback row arithmetic that the recorder depends on;
- the shell's height (no SIGWINCH height change on layout switch — only a width change).

### 4.2 Component architecture

| Component | Owner | Real data source (no invented state) | P2-A scope |
|---|---|---|---|
| **Frame layout** | `zet/src/window/layout.rs` (new) | window size + two toggles | Full |
| **Mission rail** | `zet/src/window/panels.rs` (new) | `App.tabs` (real PTYs), `zet_pty::shell::ShellState` (`Phase`, `last_exit`), `shell_cwd()` (`/proc/<pid>/cwd`), `zet_blocks::Recorder::blocks()` | Full — live sessions + observed commands |
| **Inspector** | `zet/src/window/panels.rs` (new) | `zet_blocks::Block` (command/output/exit/cwd/branch/duration), `zet_agent::JournalEntry` (tier/approval/executed/checkpoint), `zet_context::git_state` | Full — **evidence section only; no verification section** |
| **Living workstream** | existing `zet/src/window/mod.rs` | unchanged real PTY + grid | **Unchanged** except it now renders into `FrameLayout::term` |
| **Focus router** | `zet/src/window/mod.rs` | key events | Full |
| **Chrome primitives** | `zet-gpu` (additive) | — | `draw_terminal_in`, `draw_fill`, `draw_text_row`, `draw_rule` |
| **Colour tokens** | `zet-gpu` existing `const Rgba` | — | Reused verbatim; no theme system in P2-A |

### 4.3 What the panels show — and what they must never show

This is the honesty contract. Every cell rendered in the rail or inspector maps to exactly one of:

| Provenance | Meaning | Example |
|---|---|---|
| `Observed` | Read directly from a live source | exit code from OSC 133 `D;<code>`; command text from a recorded `Block` |
| `Inferred` | Derived by a local, stated rule | "failed" = `exit_code != 0`; "running" = `ShellState::phase == Running` |
| `Linked` | An explicit user action created the association | **P2-A: none.** Mission linking lands in P3 |
| `Unknown` | No source. Rendered `—`, never a value | `Block.git_branch` is `""` when not in a repo → render `—`, never "main" |

**Hard prohibitions for P2-A (each maps to a charter non-negotiable):**

1. **No verification section.** ZET cannot verify anything today — there is no test runner
   integration and no CI provider. The approved mockup shows verification cards. P2-A must not
   render that section at all — not as empty, not as "0 verified", not as a placeholder. An
   absent section is honest; a green tick is a lie. Verification is P5.
2. **No agent progress, task counts, percentages, time-saved, health scores, or sparklines.**
   There is no telemetry source for any of these.
3. **No mission "completion" state.** A mission in P2-A is a *grouping over real sessions and real
   recorded commands*, never a task list with a progress bar.
4. **No synthetic sessions.** The rail lists the real tabs, and only the real tabs.
5. **No panel that requires the terminal.** Charter: "Avoid … requiring panels to use the terminal."

**Panel content, precisely:**

*Mission rail (left)* — three stacked sections, all real:

```
┌ ZET ────────────────┐   header: product name + layout mode
│ SESSIONS            │   one row per open tab:
│  ▸ 1 zet            │     index, cwd tail, shell name
│    2 zet            │     state: live / exited  (Inferred from session+dead)
│    3 zet (exit 1)   │     last exit code (Observed) or "—"
│ OBSERVED COMMANDS   │   last N recorded blocks for the active session:
│  cargo test    0    │     command, exit code, duration
│  rg foo       127   │   (Observed from the block store)
│ ─────────────────── │
│ bash · ~/zet        │   footer: real cwd (Observed)
└─────────────────────┘
```

*Inspector (right)* — two sections, both real, and **no verification section**:

```
┌ SESSION ────────────┐   header: session id, cwd, git branch (or "—")
│ FAILED TEST         │   section: observed commands whose exit != 0
│  1 cargo test  → 1  │     (Inferred selection of Observed rows)
│  2 rg foo      → 127│
│ DECISIONS           │   section: real zet_agent::JournalEntry rows
│  T2 git push  ✓     │     tier, approved, executed, checkpoint present
│ ─────────────────── │
│ (no verification)    │   ABSENT — not rendered
└─────────────────────┘
```

If there has been no agent turn, the **DECISIONS section is absent** — not "no decisions".
"No agent turn yet" and "the agent made no decisions" are different facts.

### 4.4 Top chrome — deliberately unchanged

The tab bar keeps its current rule: row 0 **only** when there is more than one tab
(`crates/zet/src/window/mod.rs:832-834`). P2-A does not add a permanent top row, because:

- it would change the terminal's height by one row in the single-tab case, invalidating the
  startup / `cat 100k` baselines for a cosmetic gain;
- it would change the PTY height (SIGWINCH) on every layout switch.

The charter's "Status / Lightweight" bar is satisfied by the **rail footer** and the **inspector
header** (real cwd, real branch, real model spec when one is configured) rather than by a new
screen row. A dedicated status row is a P2-B candidate once a theme/token system exists.

### 4.5 Focus model

```rust
// crates/zet/src/window/mod.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Terminal,   // default: every key goes to the PTY, exactly as today
    Rail,
    Inspector,
}
```

| Focus | Keys | Guarantee |
|---|---|---|
| `Terminal` | **every** key behaves exactly as today | Zero regression risk. This is the default and the only focus P2-A returns to automatically. |
| `Rail` | `Up`/`Down` move the selection, `Enter` activates, `Esc` returns to the terminal | Typing must **never** leak to the PTY. |
| `Inspector` | `Up`/`Down` move the selection, `Enter` activates, `Esc` returns to the terminal | Same. |

The modal precedent already exists in the codebase: scrollback search captures the keyboard
(`crates/zet/src/window/mod.rs:1431-1455`). P2-A's focus router follows exactly that pattern —
a branch in the key handler that returns before `translate()`.

**Mouse:** clicking the rail focuses the rail and moves its selection; clicking the inspector
focuses the inspector; clicking the terminal focuses the terminal and starts a selection exactly
as today. The terminal's mouse→cell mapping must become **viewport-relative** (§5.3).

### 4.6 Layout switching must not lose state

The charter requires that switching layouts does not lose input, sessions, or scrollback. The
guarantees P2-A provides, and how each is verified:

| Guarantee | Mechanism | Test |
|---|---|---|
| Sessions survive | The tab list and every `Session` are untouched by layout changes | §8.2 |
| Scrollback survives | `Terminal::resize` explicitly leaves `self.scrollback` untouched (`zet-term/src/lib.rs:729-757`) | §8.2 |
| Typed input survives | The pending line lives in the recorder's absolute-row state and the shell's own line discipline; a width change makes the shell redraw the line, it does not discard it | §8.2 (live) |
| Agent turns survive | `Tab::agent_busy` + the exec channels are untouched; a width change sends SIGWINCH, which the shell handles | §8.2 |
| Pending approvals stay visible | Approvals are real `zet_policy` decisions enforced in the agent path, not a UI overlay; the inspector renders them from `JournalEntry` | §8.2 |

**And the honest caveat** — see §7.3 and Blocker B2: narrowing the viewport truncates the right
edge of the *live* rows, because `zet-term` does not reflow. The full text of every recorded
command is preserved in the block store (`zet_blocks::Block.output`), so nothing is permanently
lost, but the live view does lose the right edge until the shell repaints. This is the same
behaviour ZET already has when the *window* is narrowed.

---

## 5. Implementation path — the smallest safe Rust/wgpu change

### 5.1 Design rules

1. **No new crates.** Chrome state lives in `crates/zet/src/window/`. `zet-gpu` stays a pure
   renderer with no window/agent state (A4's binding constraint, and the charter's).
2. **No new dependencies.** The panel chrome reuses the terminal's monospace cell grid and the
   existing glyph atlas. No shaping crate, no UI framework, no image assets.
3. **No shader, pipeline, or surface changes.** The existing pipeline already draws arbitrary
   textured quads at arbitrary cell positions.
4. **Additive renderer API.** Existing functions keep working; new ones are added. The two
   existing functions whose signature must change are flagged in §5.4 (AGENTS.md: "Never silently
   change another crate's public interface — flag it instead").
5. **The grid stays cell-quantised.** One cell grid for the whole window: terminal and panels
   share cell metrics, so everything aligns and nothing needs a second font path.

### 5.2 `zet-gpu` — four additions

```rust
// crates/zet-gpu/src/renderer.rs

/// A cell-aligned rectangle in the window grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport { pub x: u32, pub y: u32, pub cols: u32, pub rows: u32 }

impl Renderer {
    /// Generalised terminal draw: the grid renders into `vp`, not the
    /// window origin. `draw_terminal` becomes a thin wrapper (origin
    /// x = 0), so today's behaviour is unchanged.
    pub fn draw_terminal_in(&mut self, term: &zet_term::Terminal, vp: Viewport,
                            sel: Option<crate::selection::Selection>);

    /// Solid background rectangle, cell-aligned.
    pub fn draw_fill(&mut self, vp: Viewport, color: Rgba);

    /// One row of text at (vp.x + col, vp.y + row), clipped to vp.cols.
    /// Reuses the existing glyph cache; no shaping change.
    pub fn draw_text_row(&mut self, vp: Viewport, row: u32, col: u32,
                         text: &str, ink: Rgba);

    /// A 1-cell vertical rule at column `x` spanning rows [y0, y1).
    pub fn draw_rule(&mut self, x: u32, y0: u32, y1: u32, color: Rgba);
}
```

All four are pure data-in/data-out and reuse the existing `cell_quad` machinery and `bg_slot`.
`draw_fill` and `draw_rule` need no glyphs at all (the background slot is already a solid texel).

**Staging order (hard contract, unchanged):**

```
r.draw_terminal_in(&term, layout.term, sel);   // 1. clears quads — MUST be first
r.draw_fill(rail_vp, RAIL_BG);                   // 2. chrome (additive)
r.draw_text_row(rail_vp, …);                     // 3. rail content
r.draw_fill(inspector_vp, INSPECTOR_BG);         // 4.
r.draw_text_row(inspector_vp, …);                // 5.
if bar > 0 { r.draw_tabbar(&labels, active, cols); }   // 6. unchanged
if let Some((cx, cy, g)) = ghost { r.draw_ghost(layout.term, cy + bar, cx, &g); } // 7.
if let Some((hits, cur)) = &search { r.draw_search(&term, hits, *cur, layout.term); } // 8.
r.render();                                      // 9. ONE draw call
```

Panels are drawn **after** the terminal and **before** the overlays, so the tab bar, ghost and
search highlights still sit on top of the terminal region exactly as today.

### 5.3 `zet` (composition root) — the changes

```rust
// crates/zet/src/window/mod.rs
struct App {
    …
    layout: Layout,             // rail/inspector visibility
    mode: LayoutMode,           // named preset (for the title/status)
    focus: Focus,               // Terminal | Rail | Inspector
    rail_sel: usize,            // selected row in the rail
    insp_sel: usize,            // selected row in the inspector
    chrome: ChromeCache,        // cached panel snapshot + dirty flag (§7.2)
}
```

Four behavioural edits, all in `crates/zet/src/window/mod.rs`:

1. **`fit_all()` becomes viewport-aware** (currently `:841-849`):

   ```rust
   fn fit_all(&mut self) {
       let vp = self.frame_layout().term;                 // NEW: resolved viewport
       let (cols, rows) = (vp.cols as usize, vp.rows as usize);
       for tab in &mut self.tabs {
           tab.term.resize(cols, rows);
           if let Some(s) = &tab.session {
               let _ = s.resize(cols.min(32767) as u16, rows.min(32767) as u16);
           }
       }
   }
   ```
   This is the correctness keystone: **the PTY width equals the terminal viewport width.**

2. **The grid offset becomes viewport-aware.** `draw_terminal_in(term, vp, sel)` places the grid
   at `vp.x`, so the mouse mapping must change too:

   ```rust
   fn mouse_cell(&self) -> Option<zet_gpu::selection::CellPos> {
       let (x, y) = self.mouse_px?;
       let r = self.renderer.as_ref()?;
       let (cw, ch) = r.cell_px();
       let vp = self.frame_layout().term;
       // Pixel → cell, relative to the terminal viewport, not the window.
       let x = x - vp.x as f64 * cw as f64;      // NEW
       let y = y - (vp.y + self.bar_rows()) as f64 * ch as f64;
       if x < 0.0 { return None; }                // NEW: click was in a panel
       Some(pix_to_cell(x, y, cw, ch, vp.cols as usize, vp.rows as usize,
                        self.tabs[self.active].term.visible_base()))
   }
   ```
   `pix_to_cell` itself needs no change — it is already viewport-relative once the origin is
   subtracted. The `x < 0.0` guard is what stops a rail click from selecting terminal cells.

3. **A focus branch in the key handler**, mirroring the existing search-mode branch
   (`:1431-1455`): when `focus != Terminal`, navigation keys are consumed and `translate()` is
   never reached.

4. **A panel snapshot built once per frame from cached data**, then handed to the renderer.

### 5.4 Public-interface changes to `zet-gpu` (flagged, per AGENTS.md)

| Item | Change | Why it cannot be avoided | Mitigation |
|---|---|---|---|
| `Renderer::draw_search` | signature gains a `Viewport` instead of `y_off: u32` | search hits must be highlighted in the terminal viewport, not at window x=0 | keep a `y_off`-compatible wrapper if the auditor prefers zero churn |
| `Renderer::draw_ghost` | signature gains a `Viewport` | the ghost sits in the terminal viewport | same |
| `Renderer::draw_terminal` | **kept**, becomes a wrapper over `draw_terminal_in` | no churn for existing callers/tests | — |
| New public items | `Viewport`, `draw_terminal_in`, `draw_fill`, `draw_text_row`, `draw_rule` | additive | — |

`zet-gpu`'s tests are all pure (no GPU device) — `crates/zet-gpu/tests/*.rs` — so every new
primitive must have its **coordinate math extracted into a pure free function** and unit-tested
there, exactly as `wrap_ghost` already is (`renderer.rs:678-704`).

---

## 6. State model — what exists, what must be invented (and what must not be)

### 6.1 Reuse map — existing types, no new telemetry

| Panel element | Existing type | Source of truth | Provenance |
|---|---|---|---|
| Session row | `zet_pty::Session` (`alive`/`dead`) | `Tab.dead`, `Session::try_exit()` | Observed |
| Session cwd | `String` | `shell_cwd(&tab.session)` — `/proc/<pid>/cwd` (`:280-290`) | Observed |
| Session exit code | `Option<u32>` | `zet_pty::shell::ShellState::last_exit()` — OSC 133 `D;<code>` | Observed |
| Session phase | `zet_pty::shell::Phase` | `ShellState::phase()` — OSC 133 `B`/`C`/`D` | Observed |
| Observed command | `zet_blocks::Block` | `Recorder::blocks()` → SQLite | Observed |
| Command exit / duration / branch | `Block.exit_code` / `.duration_ms` / `.git_branch` | recorded at the `D` marker | Observed |
| Branch when unknown | `""` | `Block.git_branch` is empty when there is no branch to name | Unknown → `—` |
| Decision row | `zet_agent::JournalEntry` | `Tab.last_journal` (loaded at spawn, updated per turn) | Observed |
| Tier / approval / executed | `JournalEntry.tier` / `.approval` / `.approved` / `.executed` | `zet_policy` + the agent | Observed |
| Undo availability | `Option<zet_agent::Checkpoint>` | `JournalEntry.checkpoint` | Observed |
| Git state | `zet_context::GitState { branch, dirty }` | `zet_context::git_state(cwd)` | Observed |
| Model / provider | `Option<String>` | `App.env_spec` / `App.startup_spec` / `zet_models::keys::resolve_inline_spec` | Observed |
| Scrollback hits | `Vec<(usize,usize,usize)>` | `Terminal::search_matches` | Observed |
| **Verification receipt** | **does not exist** | — | **P5. Never faked.** |
| **Agent progress / mission health** | **does not exist** | — | **Never.** |

### 6.2 New types introduced by P2-A (all presentational, none carrying new facts)

```rust
// crates/zet/src/window/panels.rs
pub enum Provenance { Observed, Inferred, Linked, Unknown }

/// One rendered row in a panel. `value` is display text; `provenance`
/// drives the (optional) tag so the UI never implies more certainty
/// than the source supports.
pub struct PanelRow {
    pub kind: RowKind,
    pub label: String,
    pub value: String,
    pub provenance: Provenance,
}

pub enum RowKind {
    SectionHeader,
    Session,
    Command,
    Decision,
    Status,
    Empty,          // "—" for an unknown value
}
```

`Provenance` is the mechanism that makes the charter's "honest provenance" rule mechanical rather
than aspirational: a row whose source was absent is `Unknown` and renders `—`. A future reviewer
can audit the mapping from source to cell.

### 6.3 What P2-A deliberately does **not** introduce

- No `Mission` struct that pretends to be a task list. In P2-A a "mission" is **the set of live
  sessions in this window** plus the real recorded commands they produced — a *view*, not a new
  domain object.
- No `WorkEvent`, `ArtifactRef`, `EvidenceLink`, `ApprovalState`, or `VerificationReceipt` types.
  The charter puts these in **P1/P3**. Defining them now, without a consumer, would be inventing
  a schema the architect has not approved. **Blocker B3.**
- No persistence. Nothing is written to disk by the panels. The block store already persists
  commands; the panels read it.

### 6.4 Charter-to-implementation traceability

| Charter requirement | P2-A answer |
|---|---|
| "Use real existing PTY/session data for mission and inspector example" | §6.1 — sessions, exit codes, recorded commands, journal entries |
| "No hardcoded simulated work progress" | §6.3 — no progress, ever |
| "No synthetic sessions, tests, agent telemetry, benchmark figures, approval success or verification claims" | §6.2 (`Unknown` → `—`), §4.3 prohibitions |
| "Terminal input is never blocked behind the overlay" | panels are side regions, never overlays over the terminal; the PTY is never paused |
| "Pending approvals stay visible and cannot be bypassed by presentation mode switches" | approvals are enforced in `zet_policy`/the agent path, not in the UI; the inspector *renders* them |
| "Useful without AI" | with no model configured, the rail and inspector still work — they read the block store and the shell |
| "One live native app, not different runtimes" | §4.1 — `LayoutMode` is a preset over two booleans on one `App` |

---

## 7. Keyboard conflicts, resizing, and performance risk

### 7.1 Keyboard conflict audit

**Existing bindings that P2-A must not break** (`crates/zet-gpu/src/input.rs`, `docs/SHORTCUTS.md`):

| Keys | Owner | P2-A action |
|---|---|---|
| `Ctrl+Shift+V`, `Shift+Ins` | paste | untouched |
| `Ctrl+Shift+C` | copy selection | untouched |
| `Ctrl+Shift+A` | select all | untouched |
| `Ctrl+Shift+T` / `Ctrl+Shift+W` | new / close tab | untouched |
| `Ctrl+Shift+F` | scrollback find | untouched |
| `Ctrl+PgDn` / `Ctrl+PgUp` | cycle tab | untouched |
| `Ctrl+G` | agent goal mode | **untouched — the charter explicitly flagged this one** |
| `F12` | undo last checkpoint | untouched |
| `Ctrl+C` | SIGINT (the only key that reaches the shell during an agent turn) | untouched |
| `Esc` | dismiss ghost / cancel / exit goal mode | untouched in `Focus::Terminal`; in panel focus it returns to the terminal |
| `Tab`, `Enter`, `Backspace`, `Del` | shell | untouched |
| bare `Ctrl+<letter>` | **translated to 0x01–0x1a and sent to the PTY** | **the whole reason P2-A does not use bare `Ctrl+<letter>`** |

**Proposed P2-A bindings** — all in the `Ctrl+Shift+` family that the codebase already owns:

| Keys | Action | Conflict analysis |
|---|---|---|
| `Ctrl+Shift+M` | toggle the mission rail | `Ctrl+Shift+M` is not bound by bash/zsh/readline, vim, tmux, or common terminals. Lowest-conflict letter in the family. |
| `Ctrl+Shift+I` | toggle the inspector | **Residual risk:** iTerm2 binds `Ctrl+Shift+I` (tab focus in some builds) and kitty/GNOME Terminal may use it for "copy as HTML"-adjacent actions. Mitigated because ZET intercepts **before** `translate()`, so the key never reaches the shell — but it *is* taken from the hosted app. **Alt binding offered: `Ctrl+Shift+E`.** |
| `Ctrl+Shift+L` | cycle layout (Terminal Focus → Workspace → Mission Control) | `Ctrl+Shift+L` is unbound in shells/readline; vim uses `Ctrl+W`/`Ctrl+L` (clear screen) but not the `Shift` variant. |

**Explicitly rejected alternatives:**

| Candidate | Why rejected |
|---|---|
| `Ctrl+.` for layout cycling (the old `docs/ZET-DESIGN.md:65` "mode" key) | `Ctrl+.` is **readline's `repeat-change`** and vim's repeat-substitute. Stealing it would break line editing inside ZET — unacceptable for a terminal. |
| `Alt+<letter>` | `Alt` is currently **dropped** by `translate()` (`crates/zet-gpu/src/input.rs:127-162`), so `Alt`-chords are not reliably deliverable. Fixing Alt handling is out of P2-A scope. |
| `F1`–`F12` | Heavily used by TUI apps (`F1` help, `F5` reload, `F10` menu). `F12` is already ZET's undo key. |
| `Ctrl+P` / `Ctrl+;` (old spec §5.2) | `Ctrl+P` is readline's *previous command*; `Ctrl+;` is a completion key. Both would break shell editing. |

**Rule adopted:** the `Ctrl+Shift+<letter>` namespace is reserved for ZET chrome, and no bare
`Ctrl+<letter>` chord is ever taken. Full remapping is a P6 item (`docs/ZET-DESIGN.md:197`
promises it; there is no config surface for it today).

### 7.2 Resizing behaviour

| Trigger | Behaviour |
|---|---|
| Window resize | `cols`/`rows` recomputed (existing `:1617-1629`), `frame_layout()` re-resolved, `fit_all()` resizes every tab's grid **and PTY** to the terminal viewport |
| Rail toggle | `frame_layout()` re-resolved → terminal viewport narrows by 24 cols → `fit_all()` → PTY `SIGWINCH` |
| Inspector toggle | same, by 32 cols |
| Layout cycle | both toggles change in one step → one `fit_all()` → one `SIGWINCH` |
| Window too narrow for the requested panels | panels are refused by the deterministic rules in §4.1; the terminal keeps ≥40 cols; a `Degraded` reason is available for the status line (**P2-B**; P2-A shows a one-line notice on the prompt) |
| Terminal narrower than `MIN_TERM_COLS` even with both panels collapsed | the terminal takes all remaining columns; panels stay collapsed |

**Resize storm control:** the existing code already resizes the PTY on every `Resized` event.
P2-A adds no new resize source beyond the toggles, so the SIGWINCH rate is unchanged.

**Panel snapshot caching.** Rebuilding the panel content every frame would mean a SQLite read per
frame. Instead:

```rust
// crates/zet/src/window/panels.rs
pub struct ChromeCache {
    pub snapshot: Option<ChromeSnapshot>,
    dirty: bool,
}
```

`dirty` is set when: a block is recorded, the active tab changes, the agent journal changes, the
selection moves, the layout changes, or the window resizes. The snapshot is rebuilt **at most once
per frame** and only when dirty. **Rule: no SQLite access on the keystroke path.** The block list
is read through `Recorder::blocks()` at most once per 250 ms heartbeat, not per key.

### 7.3 Performance risk register

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | **Viewport/PTY desync** — PTY width ≠ drawn width | Medium | Critical (broken transcript) | §5.3 change 1 is the keystone; §8.2 asserts `session.cols == term.cols == layout.term.cols` after every layout change |
| R2 | Live-row truncation on narrowing (no reflow) | **Certain** | Medium | Accept + document (§7.4, Blocker B2). Full output survives in the block store. Reflow is a separate `zet-term` change |
| R3 | Panel content rebuild per frame (SQLite) | Medium | High (input latency gate) | Dirty-flag cache; no DB on the key path (§7.2) |
| R4 | Quad-count growth from panel fills | Medium | Medium | Fill only the rows that carry content, not the whole column; cap visible rows per panel |
| R5 | Glyph atlas thrash (256 slots, wholesale reset) | Low–Medium | Medium (raster storm) | Fixed, ASCII-first panel glyph vocabulary; ~20 distinct glyphs max (§7.4) |
| R6 | Idle CPU regression from the 4 Hz heartbeat now also staging panels | Medium | Low–Medium | Panels are cached; the per-frame cost is quad staging of ~1–2k quads |
| R7 | Startup regression from new modules | Low | Medium (300 ms gate) | New modules are zero-cost until first use; no new I/O on the init path; `zet-gpu` gets no new device resources |
| R8 | Mouse mapping drift after the viewport change | Medium | Medium | One pure helper `viewport_hit_test(px, py, vp, cell_px) -> Option<CellPos>` with unit tests; used by both selection and panel focus |
| R9 | Scrollback/recorder desync if a scrollback row is ever inserted | Low | High | P2-A **never** inserts scrollback rows; `Terminal::resize` is used as-is |
| R10 | Panel focus swallowing keys the shell needs | Medium | High | Focus returns to `Terminal` on `Esc`, on any click in the terminal, and on the next `Ctrl+C` |

### 7.4 Glyph and colour budget

- **Colours:** P2-A reuses the existing `const Rgba` values verbatim (`renderer.rs:67-126`):
  `TABBAR_BG`, `TAB_ACTIVE_BG`, `TAB_DIM_INK`, `GHOST_RGBA`, `SEL_RGBA`. Panel chrome adds
  **no new colours** in P2-A — the rail uses the tab-bar colours, the inspector uses the
  selection/ghost inks. A token system is P2-B.
- **Glyphs:** ASCII plus at most ~6 symbols (`▸ ● ─ │ ✓ ✗`). Total distinct glyphs stays far
  below the 256-slot atlas, so no reset storm (R5).
- **No images, no icons, no SVG, no fonts beyond the terminal's.**

### 7.5 Performance budgets for P2-A (all measured against the P0 baseline)

| Gate | Budget | Method |
|---|---|---|
| Cold startup (exec → first prompt frame) | **< 300 ms, no regression > 10 %** | `ZET_PERF=1`, `first_frame_ms`, 3 runs |
| Input latency (key → pixel, warm) | **< 3 ms, no regression > 10 %** | `ZET_PERF=1`, `input_latency_us`, median of 14 |
| Idle RSS | **< 100 MB** | `ps`/`smaps`, steady state, all three layouts |
| `cat 100k lines` | **< 0.9 s** | `ZET_PERF=1`, `cmd_settle_ms` |
| Idle CPU | no measurable increase in the 4 Hz heartbeat | `top`/`pidstat` over 60 s idle, per layout |
| Layout switch | < 16 ms to the first painted frame, single `SIGWINCH` | manual + `ZET_PERF` marks |

The baseline is captured **before** P2-A and re-measured **in all three layouts**. A regression
over 10 % on any gate blocks the slice.

---

## 8. Testing strategy

### 8.1 Unit tests (pure, no GPU, no window) — the bulk of the coverage

| Module | Test | Assertion |
|---|---|---|
| `layout.rs` | `terminal_focus_is_the_full_width_grid` | `TerminalFocus` → `term.x == 0`, `term.cols == cols` — byte-identical to today |
| | `rail_and_inspector_split_the_columns` | `term.cols == cols - 24 - 32`, `term.x == 24`, `inspector.x == 24 + term.cols` |
| | `inspector_is_refused_before_the_rail` | narrow window → inspector collapses first, rail kept |
| | `both_are_refused_below_the_minimum` | `cols < 40` → terminal takes everything, `Degraded::BothRefused` |
| | `terminal_never_drops_below_the_minimum` | property test over `cols ∈ 20..=500` |
| | `mode_presets_round_trip` | `LayoutMode::next()` cycles exactly three states |
| `panels.rs` | `empty_git_branch_renders_as_unknown` | `Block.git_branch == ""` → `Provenance::Unknown`, value `"—"` |
| | `failed_command_is_inferred_from_the_exit_code` | `exit_code != 0` → `Provenance::Inferred` |
| | `no_decisions_section_without_an_agent_turn` | empty journal → the section is absent, not "no decisions" |
| | `no_verification_section_is_ever_produced` | the snapshot type has **no** verification field — a compile-time guarantee |
| | `live_sessions_come_only_from_real_tabs` | rail row count == `App.tabs.len()` |
| `renderer.rs` | `viewport_origin_offsets_the_grid` | pure `viewport_quads()` helper: cell (0,0) of a viewport at x=24 lands at pixel `24*cell_w` |
| | `fill_covers_exactly_the_viewport_cells` | pure helper: quad count == `vp.cols * vp.rows` |
| | `text_row_clips_at_the_viewport_edge` | a 40-char string in a 10-col viewport yields 10 cells |
| `input.rs` | `new_chords_do_not_shadow_existing_ones` | the six existing chord predicates still return `false` for the three new chords |
| | `ctrl_shift_m_i_l_are_recognised` | new predicates true for their chords, false for plain letters and bare `Ctrl+<letter>` |
| | `bare_ctrl_letter_still_becomes_a_control_byte` | `translate()` unchanged — no regression |

### 8.2 Integration tests (real PTY, no GPU)

| Test | Assertion |
|---|---|
| `layout_switch_keeps_the_sessions` | N tabs before, N tabs after; each `Session` still alive |
| `layout_switch_keeps_the_scrollback` | `term.scrollback_len()` unchanged across a rail toggle |
| `layout_switch_resizes_the_pty_to_the_viewport` | after a toggle, `session.cols() == term.cols() == layout.term.cols` — **the R1 regression guard** |
| `narrowing_truncates_but_preserves_the_recorded_output` | a 200-char command: the live grid is truncated, and `recorder.blocks()` still holds the full text |
| `agent_turn_survives_a_layout_switch` | start an agent turn, toggle the rail, the turn still completes and the journal still lands |
| `focus_rail_does_not_leak_keys_to_the_pty` | with the rail focused, typing `ls` produces **no** PTY bytes; after `Esc`, it does |
| `esc_returns_focus_to_the_terminal` | focus round-trip |
| `click_in_the_rail_does_not_select_terminal_cells` | a rail click leaves `selection == None` |

### 8.3 Visual regression

There is **no framebuffer readback today** (§2.2). The roadmap's P2 exit criterion ("screenshot
comparison to approved design") therefore needs one. P2-A slice 6 adds a minimal, env-gated
readback path: copy the swapchain texture to a buffer and write a PNG. It is test-only, costs
nothing when disabled, and is the precondition for any honest visual audit.

Manual review against the approved mockup (a **design north star, not a pixel lock**, per
`docs/design/ZET-FLAGSHIP-UI-REFERENCE.md`): proportions, hierarchy, restraint, and — critically —
that nothing in the UI claims a fact the app cannot back.

### 8.4 CI

The four existing gates remain the definition of done: `fmt --check`, `check --workspace
--all-targets`, `clippy -D warnings`, `test --workspace`. New code must pass `clippy -D warnings`
with no `unwrap()` in non-test paths and no `todo!()` / stubs (AGENTS.md:69-77).

---

## 9. Implementation sequence — P2-A slices

Each slice is a separate PR. Each ends green. Each is independently revertible.

| # | Slice | Files | Depends on | Exit criteria |
|---|---|---|---|---|
| **0** | **Merge the baseline** (owner action) | — | — | #13 merged → `master` green; #14 rebased onto it and green. **Not an implementer action.** |
| **1** | **Layout model** | `crates/zet/src/window/layout.rs` (new), `crates/zet/src/window/mod.rs` | 0 | `FrameLayout` resolves correctly for every width; the whole §8.1 layout suite is green. **No rendering change yet** — the app still draws one grid. |
| **2** | **Viewport-aware grid + PTY** | `crates/zet/src/window/mod.rs`, `crates/zet-gpu/src/renderer.rs` (`Viewport`, `draw_terminal_in`), `crates/zet-gpu/src/input.rs` (signatures) | 1 | The terminal renders into a sub-viewport with the **default layout still full-width** (no visible change). `draw_terminal` is a wrapper; no behaviour change. PTY == viewport width. |
| **3** | **Chrome primitives** | `crates/zet-gpu/src/renderer.rs` (`draw_fill`, `draw_text_row`, `draw_rule`), pure helpers + tests | 2 | Primitives unit-tested without a GPU; the app still draws nothing new. |
| **4** | **Mission rail** | `crates/zet/src/window/panels.rs` (new), `crates/zet/src/window/mod.rs` | 2, 3 | The rail renders live sessions + observed commands from real data. `Ctrl+Shift+M` toggles it. Scrollback/session/PTY assertions from §8.2 are green. |
| **5** | **Inspector + focus model** | `crates/zet/src/window/panels.rs`, `crates/zet/src/window/mod.rs`, `crates/zet-gpu/src/input.rs` | 4 | The inspector renders observed commands + real decisions. **No verification section.** Focus routing: rail/inspector/terminal, `Esc` returns, no key leaks to the PTY. `Ctrl+Shift+I` / `Ctrl+Shift+L` work. |
| **6** | **Screenshot readback** | `crates/zet-gpu/src/renderer.rs` (env-gated) | 3 | A PNG can be captured from a running window for visual audit. Test-only path. |
| **7** | **Performance + compatibility pass** | — | 4, 5 | All §7.5 gates re-measured in all three layouts with no regression > 10 %. Compatibility smoke: bash, zsh, `less`, `vim`, `tmux`, `ssh`, CJK, wide output, 100k lines. |

**Ordering rationale:** slices 1–3 change *geometry* only and are individually invisible, so the
highest-risk change (the viewport/PTY coupling, R1) lands before any chrome exists. The chrome
slices (4–5) then add only content, which is the cheapest thing to get wrong.

**Total new code estimate:** ~700–900 lines, of which ~350 is `zet-gpu` primitives and their
tests, ~400 is `layout.rs` + `panels.rs`, ~150 is the `zet` integration. No new dependencies.

---

## 10. Explicitly out of scope for P2-A

Splits; command palette; theme/token system; animation/tweens; grid-anchored block headers
(A4's `BlockView`); `Mission` persistence and linking; verification receipts; CI provider
integration; keybinding remapping config; agent journal streaming (A4's `with_journal_sink`);
packaging. Each is either P2-B, P3, P5, or P6 in the roadmap.

---

## 11. Blockers requiring architect (GPT-6) review

| # | Blocker | Why it needs the architect | Proposed resolution |
|---|---|---|---|
| **B1** | `master` is red; #13 is green but unmerged; #14 is red because it predates #13 | Release state is the owner's call, and P2-A cannot start on a red baseline | Owner merges #13 → owner rebases #14 → P2-A starts. **This plan changes nothing.** |
| **B2** | **`zet-term` does not reflow.** Narrowing the terminal for a rail truncates the live grid's right edge (`crates/zet-term/src/lib.rs:729-757`); `Cell` has no soft-wrap flag; the recorder keys on absolute rows so inserting scrollback rows mid-session would desync blocks | This is a **core terminal** change with a real blast radius. It is *not* "the smallest safe path" and it is not P2-A's job | **P2-A accepts the truncation** and documents it honestly (the full text remains in the block store). **P2-B (or a standalone `zet-term` PR):** add a soft-wrap flag to `Cell`, implement xterm-style reflow, and re-anchor the recorder's absolute rows — with its own test corpus. **Architect must confirm the P2-A/P2-B split.** |
| **B3** | The charter's `Mission` / `WorkEvent` / `ArtifactRef` / `EvidenceLink` / `ApprovalState` / `VerificationReceipt` schema (P1/P3) does not exist | Defining it now, without a consumer, would pre-empt the P1 contract and risk inventing telemetry | P2-A introduces **none** of them; the rail/inspector are read-only views over existing types (§6.1). The schema is proposed in **P1-B**, and P3 consumes it |
| **B4** | `Ctrl+Shift+I` (inspector) is bound by some other terminals (iTerm2/kitty families) | Taking a chord from a hosted TUI is a compatibility decision, not an implementation detail | Recommended: `Ctrl+Shift+I` primary, `Ctrl+Shift+E` alternative, both listed in `docs/SHORTCUTS.md`; the `Ctrl+Shift+` namespace is reserved for chrome. Architect confirms |
| **B5** | No framebuffer readback exists, but the roadmap's P2 exit needs a screenshot comparison | Adding a GPU readback path is a new capability in `zet-gpu` | Proposed as slice 6, env-gated and test-only. Architect confirms it is in P2-A's scope rather than P2-B |
| **B6** | The charter's "Status / Lightweight" bar is realised inside the rail/inspector rather than as a dedicated row | A dedicated row changes the terminal height and the PTY `SIGWINCH` behaviour, invalidating the perf baselines | Proposed: no new row in P2-A (§4.4); a dedicated status row is a P2-B candidate once tokens exist |

---

## 12. Provenance of the claims in this document

Every statement about the current implementation is traceable to a file and line in the tree at
`origin/fix/ci-text-blit-ascent` (`166c204`). Every statement about CI and PR state was read from
the GitHub API on 2026-10-22. No performance figure in this document is a forecast: the budgets in
§7.5 are the **existing** gates from `AGENTS.md` and `docs/ZET-DESIGN.md`, and the P0 baseline
values (186–249 ms startup, ~1.0 ms input, 86–92 MB RSS, 192 ms `cat 100k`) are the ones already
recorded in `docs/ZET-DESIGN.md:119-127`. P2-A must **re-measure** them, not assume them.
