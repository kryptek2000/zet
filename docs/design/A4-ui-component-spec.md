> **HISTORICAL UI REFERENCE — 2026-10-22:** The approved direction is [ZET Product Charter](../ZET-PRODUCT-CHARTER.md) and [Roadmap](../ROADMAP.md). This A4 design contains reusable native component/renderer constraints, but its Ghostty × Warp styling, panel composition, block-only interaction assumptions and exact shortcuts are **not an approved final implementation specification**. Reconcile before coding. The flagship UX is the outcome-aware Mission Control workspace with a first-class real PTY, collapsible panels, truthful evidence and no required agent platform.

# A4 — Zet UI/UX Component Specification (Ghostty × Warp synthesis)

**Status:** design · **Crate ownership:** `zet` (composition root / chrome) + `zet-gpu` (pure renderer) + `zet-blocks` / `zet-agent` / `zet-policy` / `zet-suggest` / `zet-term` / `zet-pty` as data sources.
**Base:** current HEAD (A1 window-unification + A2 SSH transport complete).
**Spec cross-ref:** §2 principles, §5 UX System (three modes / key surfaces / visual identity), §6.4 safety tiers, §7 perf gates + perf rules, §8 architecture (crate list, threading, rendering pipeline, state layout).

> **Note on the `ground-truth/decisions.md` reference.** The vision file cited in the task briefing is not checked into the repository (verified: no `ground-truth/` tree, no `decisions.md` anywhere under `crates/`). This design is grounded instead in the spec's **§5 UX System** (the authoritative UI/UX Vision, incl. §5.1 three-mode model), the real M0 implementation, and sibling designs A1 (window unification) and A2 (SSH).

---

## Decision

**Chrome lives in `crates/zet/src/window/`; the GPU renderer (`zet-gpu`) stages quads from an inert `Palette`+snapshot only — it holds no mode/agent state and never blocks on PTY or model I/O.** All eight components are expressed as *zet-owned state* that produces a per-frame snapshot fed to a small, additive set of renderer stages; blocks/splits/theme/animation are built by composing the existing `draw_terminal`/`draw_ghost`/`draw_search` contract rather than by adding new crates or giving `zet-gpu` a UI state machine.

## Context — the constraint that forced it

- §2.1 — *Terminal first, chat second.* The grid is canonical; AI renders as blocks, never hijacks the PTY. ⇒ Block/transcript content is **always the real terminal grid**; block chrome is an overlay, never a parallel DOM/PTY.
- §7 perf rules + §8 rendering pipeline — *"renderer on dedicated thread, PTY parse off UI thread … wgpu glyph atlas, incremental block virtualization (only visible blocks)"*; pipeline = *"VT grid → shaped glyph run cache → wgpu atlas → damage rects → present."* ⇒ Per-frame work is *stage visible quads only*; `draw_*` clears-and-stages, `render()` is one draw call, idle sleeps (4 Hz heartbeat).
- §2.7 / §8:139–153 — *"No new crates."* ⇒ Every component maps to an **existing** crate. A4 adds only *modules inside `crates/zet/src/window/`* and a few **pure-data** fields/methods on the existing `zet-gpu::Renderer`. No `zet-ui`, no `zet-theme` crate.
- §8 threading — *"UI/GPU thread ↔ PTY reader ↔ agent tokio runtime ↔ model streaming. All cross via lock-free rings; PTY output never blocks on model I/O."* ⇒ The render loop takes a **snapshot** of shared state under a brief borrow; the renderer never awaits.
- The render loop is already load-bearing and destructive in one spot: `Renderer::draw_terminal` calls `self.quads.clear()` (`crates/zet-gpu/src/renderer.rs:487`), so the staging order **terminal-first, overlays-after** is a hard contract (`crates/zet/src/window/mod.rs:1693-1709`): `draw_terminal → draw_tabbar → draw_ghost → draw_search → render`. A4 preserves and extends it.
- §5.3 — *"120Hz motion only for cursor/scroll; AI streaming never janks scroll"* + §7 *"120fps fixed is mutually exclusive with sleeping idle — dropped deliberately."* ⇒ Animation runs on a 120 Hz *poll* only while a tween is active; the 4 Hz idle heartbeat is untouched while idle.

**Constraint resolution (the blanks in the briefing).** `zet-gpu` stays pure renderer; all UI logic lives in **`crates/zet`** (the `window/mod.rs` composition root + `window/` modules). Blocks use the **`zet-blocks`** store; agent uses **`zet-agent`**; policy uses **`zet-policy`**; suggest uses **`zet-suggest`**. Output path: **`docs/design/A4-ui-component-spec.md`** (sibling to A1/A2).

---

## 1. Component hierarchy & ownership

| Component | Owner crate / module | Real data source | Public type(s) in use today |
|---|---|---|---|
| **Window loop** | `zet/src/window/mod.rs` (`App`, `Tab`) | all of the below | `App{pub tabs:Vec<Tab>, renderer: Option<Renderer>, ...}`, `Tab{session, term, recorder, ghost, ...}` (`mod.rs:186-217`, `:63-89`) |
| **Tab bar** | `zet/src/window/mod.rs` (chrome) | tab list, cwd | `draw_tabbar(labels, active, cols)` (`renderer.rs:555`); `tab_segment` (`selection.rs:117`) |
| **Block renderer** | `zet/src/window/blocks.rs` (new) | `zet_blocks::{Store,Block,Recorder}` | `Block{ id,session,command,output,exit_code,cwd,git_branch,duration_ms }` (`zet-blocks/src/lib.rs:27-39`); `BlockView` (new, see §4) |
| **Command palette** | `zet/src/window/palette.rs` (new) | `zet_pty::{KnownCommands, CLI_SUBCOMMANDS}`, `Store::recent`, recipes | `KnownCommands::from_env()` (`mod.rs:1853`); `Store::recent/list` (`zet-blocks/src/lib.rs:149,139`) |
| **Split manager** | `zet/src/window/splits.rs` (new) | n/a (viewport tree) — one `zet_pty::Session` + `zet_term::Terminal` per leaf | `Session::spawn`, `session.resize(cols,rows)` (`mod.rs:104-106`, `:846`) |
| **Ghost overlay** | `zet-suggest` (logic) + `zet/src/window/mod.rs` (state) | `zet_suggest::Ghost` + `PromptInput` | `Ghost::update/accept/cancel/notify/preview/waiting/visible`, `PromptInput{ text,prompt_col,prompt_row,cursor }` (`zet-suggest/src/ghost.rs`) |
| **Agent block stream** | `zet/src/window/blocks.rs` (render) + `zet-agent` (emit) | `zet_agent::JournalEntry`, `Checkpoint` | `JournalEntry{ command,tier,approval,approved,executed,exit_code,output,checkpoint }`; `Checkpoint{ head,stash }` (`zet-agent/src/lib.rs:161-186`,`854-858`) |
| **Theme** | `zet/src/window/theme.rs` (new: token parse, light/dark/auto, user TOML) + `zet-gpu::renderer::Palette` (new: resolved Rgba) | `Palette` replaces loose `const Rgba` (`renderer.rs:67-126`); §11 `config.toml` `[ui]` | `config.toml [ui]` (`ZET-DESIGN.md:184`: `font,font_size,opacity`) |
| **Animation** | `zet/src/window/anim.rs` (new: tweens, easing, 120 Hz timing) | `zet_gpu::damage::{Rect,Damage}` (real infra) | `Damage{ add, take }`, `Rect{ x,y,w,h }`; both in `damage.rs:4-65` (real) |

## 2. Render-loop integration (where UI fits in the wgpu frame)

Current (M0) sequence — **load-bearing order** because `draw_terminal` clears `quads`:

```
// crates/zet/src/window/mod.rs:1692-1712
pump()            // drain PTY bytes, record blocks, poll agent/plan/undo/ghost   (§3 event flow)
snapshot chrome   // copy: labels, sel, ghost, search, block views, split tree   (brief borrow)
r.draw_terminal(&term, cols, grid_rows, sel, bar);          // clears quads, stages grid
r.draw_blocks(&block_views, cols, bar);                     // A4: header bg + meta text, collapse dim  (NEW)
if bar > 0 { r.draw_tabbar(&labels, active, cols); }
r.draw_dividers(&dividers);                                 // A4: split divider lines                (NEW)
if let Some((cx,cy,g)) = ghost { r.draw_ghost(cy+bar, cx, &g, cols, rows); }
if let Some((hits,cur)) = &search { r.draw_search(&term, hits, *cur, bar); }
if palette.on { r.draw_palette(&entries, palette.selected, cols, rows); } // A4  (NEW)
r.render();          // ONE draw call: upload quads → render pass → present
note_frame();        // ZET_PERF probes (§7)
```

**Contract enforced:** `draw_terminal` is always first (it owns the clear). Every other `draw_*` is *additive* — it only **extends `self.quads`** with overlay quads and never touches staged cells. This is why the renderer stays pure: it is a function `Snapshot → [Vertex]`.

**Renderer extensions (zet-gpu) — pure, data-in:**
```rust
// crates/zet-gpu/src/renderer.rs — replaces the loose consts 67-126 with a struct:
pub struct Palette {
    pub screen_bg: Rgba, pub ghost_ink: Rgba, pub sel_fill: Rgba,
    pub tab_bg: Rgba, pub tab_active_bg: Rgba, pub tab_dim_ink: Rgba, pub tab_active_ink: Rgba,
    pub search_fill: Rgba, pub search_cur: Rgba,
    pub block_header_bg: Rgba, pub block_header_fg: Rgba, pub block_collapsed_dim: Rgba,
    pub block_border: Rgba, pub diff_added: Rgba, pub diff_removed: Rgba, pub doc_ink: Rgba, pub error_ink: Rgba,
    pub ansi: [Rgba; 16],            // replaces PALETTE in term_color()
    pub anim_idle_bg: Rgba,           // frame of a transitioning element
}
impl Renderer {
    pub fn apply_palette(&mut self, p: Palette);              // theme swap (or animated tween)
    pub fn draw_terminal_in(&mut self, term:&Terminal, cols:usize, rows:usize, origin:(u32,u32), sel, y_off:u32); // generalized for splits
    pub fn draw_fill(&mut self, x:u32, y:u32, w:u32, h:u32, color:Rgba);      // block header bg / collapse dim / divider / palette bg
    pub fn draw_text_row(&mut self, row:u32, col:u32, text:&str, ink:Rgba);  // block meta / palette text (no per-call char loops in App)
    pub fn draw_divider(&mut self, vertical:bool, at:u32, span:(u32,u32), color:Rgba);
    pub fn draw_palette(&mut self, rect:Rect, entries:&[PaletteEntry], selected:usize, palette:&Palette);
}
```
`draw_terminal` becomes a one-line wrapper: `draw_terminal_in(term, cols, rows, (0,0), sel, y_off)`. **No behavior change to M0.** The split generalization is the only reason `draw_terminal_in` exists — §5.3 splits must be GPU-accelerated.

**Damage-only (§8: "damage rects → present").** `zet_gpu::damage::Damage` + `Rect` already exist and are pure (`damage.rs:4-65`: `Rect {x,y,w,h}`, `Damage { add, take }`). Frame N+1: if the `Damage` set covers the full screen → `LoadOp::Clear` (current behaviour); else `LoadOp::Load` + re-stage only quads inside the dirty `Rect`s (small additive helper: `Damage::covers(&self, screen: &Rect) -> bool`). `App` computes the `Damage` set from its diffed `Snapshot` (PTY bytes → full grid clear; ghost char → 1-cell rect; animation → animating element rect). This is the infra the perf spec lists — we wire it, not rebuild it.

**120 Hz timing (§5.3 / §7).** `App::needs_draw` + `ControlFlow`. Idle = `WaitUntil(now + FRAME)` (4 Hz, `mod.rs:277,1325-1330`). While any `Tween` is `active()` → `ControlFlow::Poll` (true 120 Hz) until the last tween expires, then back to 4 Hz. Cursor/blink is the existing `cursor_visible` flip; scroll is demand-driven (`mod.rs:1640-1671` already does per-frame drag scroll). AI-streaming blocks render on arrival but **never drive the poll loop** — they settle the 4 Hz heartbeat like PTY output (§5.3 "AI streaming never janks scroll").

## 3. Data structures (block, split, tab, palette, ghost text, agent stream)

All structs live in `crates/zet/src/window/` unless noted — **wrapping existing types, never replacing them.**

### 3.1 `BlockView` (in `window/blocks.rs`) — the block renderer's snapshot

This is the bridge from `zet_blocks::Block`/`JournalEntry` to screen space. It pins a recorded block to **absolute terminal rows** so the renderer can overlay chrome without disturbing `zet_term::Terminal`'s grid.

```rust
// window/blocks.rs
pub struct BlockView {
    pub block_id: i64,                  // zet_blocks::Block.id
    /// Absolute (scrollback-relative) row of the prompt line where this
    /// command was typed. Source: zet_pty::ShellState OSC 133 `P;` marker
    /// — the prompt row the Recorder already parses (zet-blocks recorder.rs).
    pub prompt_row: usize,
    /// Absolute row range of the transcript (prompt_row+1 .. end_row).
    /// `end_row` is the row of the next block's prompt, or the grid bottom.
    pub output_rows: std::ops::Range<usize>,
    pub command: String,                // scrubbed (zet_blocks::scrub_secrets already ran)
    pub exit_code: i32,
    pub duration_ms: u64,
    pub git_branch: String,
    pub cwd: String,
    /// Agent trace handle: the JournalEntry span this block belongs to, if
    /// it was agent-typed (zet_agent::JournalEntry.command == this command).
    pub journal_idx: Option<usize>,
    pub checkpoint: Option<zet_agent::Checkpoint>,  // Some ⇒ F12-undoable
    pub collapsed: bool,
    /// Computed by zet (layout), never stored: header lines after truncation.
    pub header_lines: Vec<String>,      // e.g. [" ls -la ", "main · 42ms · exit 0 "]
}
impl BlockView {
    /// Build from a real Block + its prompt row. `end_row` filled later
    /// (depends on the next block's marker — the window knows the chain).
    pub fn from_block(b: &zet_blocks::Block, prompt_row: usize) -> Self { /* … */ }
}
```

**Why prompt-row anchoring (not row insertion).** `zet_term::Terminal` owns its own cursor/scrollback row math (`Terminal::cursor/visible_cell/visible_base` — `zet-term/src/lib.rs:490-548`); inserting header rows would desync it from the shell and renumber every scrollback address the recorder depends on. Anchoring = Warp-style header paint over the prompt row + dim-fill over output rows, the same overlay technique `draw_search`/`draw_ghost` already use on grid cells.

### 3.2 `SplitTree` (in `window/splits.rs`) — GPU-accelerated splits

A recursive viewport tree; each leaf holds its own `zet_pty::Session` + `Terminal` (one PTY per split — a split is "its own real shell", per §5.1).

```rust
// window/splits.rs
pub enum SplitKind { Horizontal, Vertical }
pub struct SplitNode {
    pub kind: SplitKind,                // Horizontal = side-by-side, Vertical = stacked
    pub ratio: f32,                     // 0..1 child size fraction; animated via Tween
    pub left:  Box<SplitChild>,
    pub right: Box<SplitChild>,
    pub divider: DividerState,          // drag position + anim handle
}
pub enum SplitChild {
    Leaf { tab_idx: usize },           // index into App::tabs (one Session+Terminal each)
    Node(Box<SplitNode>),
}
pub struct DividerState {
    pub dragging: bool,
    pub anim: Tween,                    // snap-back if released mid-grid
}
/// Screen-space viewport for one leaf; computed from the tree + cell_px.
pub struct Viewport { pub origin:(u32,u32), pub cols:usize, pub rows:usize, pub bar:bool }
impl SplitTree {
    pub fn viewports(&self, cols:usize, rows:usize) -> Vec<(usize /*tab*/, Viewport)>; // recurse
    pub fn hit(&self, x:u32,y:u32) -> Option<...>;      // drag start / focus
    pub fn drag(&mut self, delta:f32);                // live resize
}
```

**Focus follows:** moving the cursor / clicking a split sets `App.active` to that leaf's tab (single cursor model; splits are panes of tabs, not separate cursors). "Per-split agent context" = each leaf tab keeps its own `agent`/`goal`/`journal` (already per-Tab in `mod.rs:63`); the agent turn is bound to `App.active`'s leaf. **Drag-resize** = live `ratio` tween on every motion event; `focus follow` is pointer-driven (mouse enters → active). No mousewheel fan-out.

### 3.3 `Tab` (existing, grows) — §5.1 "per-tab agent context, close/duplicate/pin"

`crates/zet/src/window/mod.rs:63-89` already owns `session/term/recorder/ghost/agent_*`. A4 adds three bools + a pin flag:
```rust
// mod.rs Tab — minimal additive fields:
pub pin: bool,            // pinned tab lives leftmost; never auto-closed
pub duplicated_from: Option<usize>,  // for duplicate
```
- **Drag-reorder:** a `MouseDown` on a tab segment starts a drag; the segment paints a lifted alpha (`anim::Tween` on `tab_active_ink.a`); on drop, `App::tabs.swap` + `Tab::spawn` is **not** called (reorder, not duplicate). Uses the real `tab_segment(x, cols, n)` (`selection.rs:117`) to map pixel→tab.
- **Duplicate:** `dup_tab(i)` = `Tab::spawn` from tab `i`'s `prog+hook` + initial `ZET_SESSION` export; new tab shares no PTY (correct: a duplicate is a fresh shell, like Ghostty).
- **Close/Pin:** close = existing `close_tab` (`mod.rs:891`) gated on `!tab.pin`; last tab still closes the window; pinned tab is never the one closed by `ctrl-w` inside the shell (that's the PTY's `^W`/kill, out of scope here).

### 3.4 `PaletteEntry` + `CommandPalette` (in `window/palette.rs`)

Keyboard-only, `,,` chord dispatch (§5.2 canonical binding is `Ctrl+P`; the briefing's `,,` is the fast entry). The palette is a *modal overlay sheet* (not a block), so it renders as a `draw_palette` rect over the grid and **steals all keys** until dismissed (matches the search-mode precedent at `mod.rs:1422-1455` where `search.on` short-circuits the key router).

```rust
// window/palette.rs
pub struct PaletteEntry {
    pub title: String,        // human label (icon + text)
    pub kind: PaletteKind,    // Command | Tab | Block | Recipe | Tool | Setting | Recent
    pub payload: String,      // argv tail / block id / recipe path / setting key
}
pub enum PaletteKind { Command, Tab, Block, Recipe, Tool, Setting, Recent }
pub struct CommandPalette {
    pub on: bool,
    pub query: String,
    pub entries: Vec<PaletteEntry>,   // rebuilt on each keystroke from live sources
    pub picked: usize,                // keyboard selection
    pub fzy: FuzzyIndex,               // std-less byte subsequence match; no dep
}
impl CommandPalette {
    pub fn rebuild(&mut self, app: &App) { /* zet_pty::CLI_SUBCOMMANDS + known + Store::recent + recipes() + tabs */ }
    pub fn run(&self, entry: &PaletteEntry, app: &mut App) { /* dispatch — see §6.4 */ }
}
```
**Sources are real:** `zet_pty::CLI_SUBCOMMANDS` (`shell.rs`), `KnownCommands::from_env()` (`pty`'s `first_word`/`is_shell_word`), `Store::recent(limit)` + `Store::list(session)` (`zet-blocks`), recipes from `~/.zet/recipes/*.md` + repo `.zet/recipes/*.md` (§6.5 / §11), tabs from `App::tabs`. No new crate reads the model layer.

### 3.5 `GhostPreview` (extension of `zet_suggest::Ghost`) — §5.1 inline suggestions

`Ghost` currently exposes a single `visible() -> Option<&str>` suffix + `notify(input, text)` notice. The A4 briefing (ghost-text overlay) asks for **inline diffs / inline docs / inline errors** beyond the history/NL ghost that §5.1 defines. We extend *not replace*: add a `kind` discriminator so the renderer can tint correctly.

```rust
// zet-suggest/src/ghost.rs — minimal additive enum (Ghost keeps all existing methods)
pub enum GhostKind { History, Diff, Doc, Error, Notice }
impl Ghost {
    pub fn visible_kind(&self) -> Option<(&str, GhostKind)>;   // replaces visible() for tinting
    pub fn preview_kind(&mut self, full:String, kind:GhostKind);  // typed preview
}
```
- **Inline NL→command** = existing `notify`/`preview` + `ModelSuggester::suggest_nl` (`zet-suggest/src/lib.rs:75`).
- **Inline diff** = ghost renders the `+` lines of `tools::unified_diff` (`zet-agent/tools.rs:283`) over the prompt, tinted `Palette::diff_added` — accepted by Tab.
- **Inline docs** = a short doc string from a model/symbol look-up, tinted `Palette::doc_ink`.
- **Inline errors** = current `notify` notices (`"(model error: …)"`, `"(invalid: …)"`, `"(no command)"`) retinted `Palette::error_ink`; no behavior change to the existing notices at `mod.rs:543/580/585`.
- **Rules:** `Tab` accepts `History`/`Diff`/`Doc`; `Esc` dismisses all; all tinted `Palette::ghost_ink`-family. Ghost stays **history-instant (<50ms, §7)**; only `History` uses the store; NL/diff/doc/Error-notice are `notify`-driven. No model call touches the ghost render path.

### 3.6 Agent block stream (in `window/blocks.rs` render + `zet-agent` emit)

§5.1 Agent mode ("streams tool calls as collapsible blocks"). Today the agent runs on a thread and the window only gets the final `AgentDone` (`mod.rs:231`, `:1159`). A4 adds a **journal sink** reusing the real `JournalEntry`:

```rust
// zet-agent/src/lib.rs — additive builder arg, no new event type
impl<M: ChatModel, A: Approver> Agent<M,A> {
    pub fn with_journal_sink(mut self, tx: std::sync::mpsc::Sender<zet_agent::JournalEntry>) -> Self { self.sink = Some(tx); self }
}
// At every existing journal push site (step/step_read/step_write/step_mcp/verify),
// after self.journal.push(e.clone()): if let Some(t)=&self.sink { let _ = t.send(e.clone()); }
```
The 5 push sites already exist at every `self.journal.push(JournalEntry{..})` — representative: `lib.rs:457,566,628,811,696` (reads / writes / mcp / shell / verify, plus the denied `decide` branch). After each push, emit on the sink (`if let Some(t) = &self.sink { let _ = t.send(e.clone()); }`). Each emitted `JournalEntry` becomes one **agent-trace block** in the block panel (collapsible, tinted by tier: T0=T0-ink, T1=T1-ink, T2=T2-ink, T3=T3-ink). Native tool calls (`read_file`/`glob`/`grep`/`write_file`) produce trace blocks with **no terminal transcript** (they never hit the PTY — `step_read`/`step_write` are in-process) plus inline-diff on `write_file` (reuse `tools::unified_diff`). Shell steps (`run_shell`) **are** typed into the PTY (`PtyExec`, `mod.rs:246`) so their transcript *is* a normal `BlockView` — the trace block links to it via `block_id`. This realizes §6.1 *Propose→Plan→Execute→Verify* as streaming, selectable, undoable blocks, and satisfies §2.3 (visible block + diff + audit + undo).

The planner phase (`draft_plan`, `zet-agent/src/lib.rs:33`) is the existing `PlanDraft` channel (`mod.rs:234`); the drafted steps render as a single streaming "plan" block, then each step spawns an executor trace block. **Never hijacks the PTY frame:** trace blocks render in the *palette/side* chrome region or as non-grid overlays; the grid keeps showing the typed shell output live.

## 4. Event flow (input → PTY → block → render) — one frame, no blocking

```
[main thread/UI]                          [model thread (off UI)]            [PTY reader thread]
1. key (e.g. Enter) → translate()→bytes   start_agent/plan (std::thread::spawn)  session.write_all(bytes)
                                          (mod.rs:991,1051)                        ↑ PTY receives bytes
2. bytes → Session.write_all              Agent::run() / draft_plan
   (mod.rs:1699 / 246)                    emits JournalEntry per step            reader.read(buf) → wake.send_event(()) (mod.rs:131)
                                          → tx.send(JournalEntry)               term.feed(chunk) (mod.rs:344)
                                          ↑ recv in main loop's pump()         ShellState.observe (mod.rs:354-357)
3. RedrawRequested → pump()                RecordedBlock append (block view)        Store::append (zet-blocks/lib.rs:103)
   draw_terminal→draw_blocks→draw_tabbar     (z-order: grid first[clears quads],
    →draw_ghost→draw_search→draw_palette    then chrome overlays)                  Record: OSC 133 P;D;D markers
4. renderer.render() → present             ← block_id + prompt_row from recorder
   (ONE draw call; quads from snapshot)
```
**Threads never touch the renderer:** the PTY reader pushes `Vec<u8>` on the proxy channel that only sets `needs_draw`; the model thread pushes `JournalEntry` on a channel the main loop `try_recv`s in `pump()`. Concretely:

1. **PTY never blocks on model.** PTY reader is a `std::thread::spawn` producer (`mod.rs:120-136`) pushing `Vec<u8>` woken by `wake.send_event(())` (`mod.rs:131`); model streaming (`start_nl_request`/`start_agent`) is a *separate* `std::thread::spawn` (`mod.rs:991`, `:1051`) on its own channel. Neither touches `self.renderer`.
2. **UI logic never blocks on PTY.** `pump()` (`mod.rs:293`) drains with `try_recv` (non-blocking) and snapshots chrome before borrowing the renderer (`mod.rs:1673`).
3. **Snapshot is cheap & brief.** The `RedrawRequested` arm copies only `Copy`/owned values (`mod.rs:1675-1691`); `draw_*` re-reads `self.tabs[active].term` (the canonical grid) — no grid clone.
4. **Snapshot includes A4 surfaces.** The copy grows by: `let blocks = self.block_views(active);` (from `Store::list` + `Recorder` markers, O(visible) via §7 virtualization), `self.splits.viewports(...)`, `self.palette.snapshot()` — each a non-blocking borrow of already-cached state.


## 5. Theme system — semantic tokens → resolved `Palette`

§11 `config.toml` already has `[ui]` (`font/font_size/opacity`). A4 adds **token resolution** (§5.3 "semantic tokens"; §2.7 standards) into that file, **no separate theme crate**.

```toml
# ~/.zet/config.toml (additive, §11)
[ui]
font = "JetBrainsMono Nerd Font"; font_size = 13; opacity = 0.96
theme = "auto"        # "light" | "dark" | "auto" (follows OS via winit)  §5.3
[ui.theme_tokens]    # semantic → role, overrides the built-in palette
prompt = "#00ff88"   # ghost/inline ink
cursor = "#ff8800"
panel = "#0d1117"    # tab bar + block headers
accent = "#58a6ff"
text = "#c9d1d9"
```
`window/theme.rs`:
```rust
pub struct ThemeDef { pub base: BaseScheme, pub tokens: Tokens }
pub enum BaseScheme { Light, Dark, Auto }                 // "auto" reads winit `ColorScheme`
pub struct Tokens { pub prompt: String, pub cursor: String, pub panel: String, pub accent: String, pub text: String }
impl ThemeDef {
    /// Resolves semantic tokens → the concrete Palette the renderer paints with.
    pub fn resolve(&self, scheme: ResolvedScheme) -> zet_gpu::renderer::Palette;
}
/// Ship with two built-in schemes (current M0 dark is the base of `Dark`).
pub const BUILTIN_DARK: &str = "...";     // equals today's BG/GHOST/SEL/TAB/SEARCH consts exactly
pub const BUILTIN_LIGHT: &str = "...";   // inverted: light bg, dark ink
```
**Grounded in reality:** the M0 dark values (`renderer.rs:67-126` + the `PALETTE` in `term_color` at `:133-150`) become `BUILTIN_DARK` identically — this is a *formalization*, not a redesign. `Palette::ansi` is that same `[Rgba;16]` moved out of `term_color`. Light = same roles, inverted luminance. `auto` = `ThemeDef::Auto` → at `WindowEvent::ThemeChanged` (`winit`), re-`resolve` + `apply_palette` (a single `write_buffer` of the bg, no re-raster). User `[ui.theme_tokens]` override the chosen scheme's roles. **Acceptance:** dark default matches M0 pixel-for-pixel (`cat 100k` test still green).

## 6. Animation framework — tweens, easing, 120 Hz

`window/anim.rs`, zero new deps:
```rust
pub enum Ease { Linear, CubicOut, ExpoOut }
pub struct Tween { pub start:f32, pub end:f32, pub dur:Duration, pub ease:Ease, pub started:Option<Instant> }
impl Tween {
    pub fn active(&self, now:Instant) -> bool { now < self.started.unwrap()+self.dur }
    pub fn lerp(&self, now:Instant) -> f32 { /* ease curve at (now-started)/dur, clamped */ }
}
pub struct Animated<T> { pub value:T, pub tween:Option<Tween> }
```
**Animated properties:** `SplitNode.ratio` (live drag + snap-back), `CommandPalette.opacity` (fade open/close), `BlockView.expanded` → header height tween (expand/collapse), tab-switch alpha, search `query` char fade. Each frame the `pump`/draw step reads `tween.lerp(now)` and passes the resolved value to the renderer. **Idle = 4 Hz**; any active tween ⇒ `ControlFlow::Poll` ≤ `tween.dur` (~200 ms default) ⇒ back to 4 Hz. (§7: fixed-120 fps while sleeping idle is rejected.)

## 7. Acceptance checks (perf + functional)

Perf gates are **unchanged** (§7 release gates); the new chrome must keep them:
- **Cold startup <300ms:** chrome modules are zero-cost until first open (no PTY/model on the init path; `run()` already cold-stops at `mod.rs:1817`). ✓ by measurement: A4 adds no per-frame alloc on idle (snapshot is `Copy`/owned borrows; `Vec<BlockView>` is built only when blocks change, diffed via `Store::recent` rowid).
- **Input latency <3ms warm:** key→byte→draw path; new `draw_*` append to `quads` (O(visible blocks), virtualized) before the single `render()`. The 4 Hz heartbeat is untouched; chrome only adds to `needs_draw` frames already demanded by input.
- **`cat 100k lines <0.9s`:** proven 192ms (§7 table). Block chrome adds a header only at *recorded command boundaries* (OSC 133 `P;`), not per line — `cat` produces one output block. Virtualization (`Store::recent`/`list`, `Rect` damage) keeps O(visible) regardless of scrollback depth.
- **Inline suggest <50ms:** untouched — `Ghost::update` stays sync/history (`ghost.rs:48`); the new `GhostKind` is a paint-time tint, not an extra model call.
- **Idle RSS <100MB:** no new deps; theme is parsed TOML strings, animation is `Instant`/`f32` math, block views are built lazily and dropped (no retained DOM). `DrawFill`/`draw_text_row` add only to `quads` (same `Vec<Vertex>` as today).

Functional checks:
- `zet` opens the window (no model) → renders dark scheme identical to M0; tab bar shows `[1/1]` only when >1 tab; `Ctrl+Shift+T/N` new/cycle; `Ctrl+Shift+W` closes (last closes window).
- Type a command, run it → a `BlockView` with a header row (`command · branch · exit · duration`) appears over the prompt row; collapsing it dims the output rows; `F12` offers `undo` when `checkpoint: Some`.
- `Ctrl+G` goal → plan block renders, Enter on a real plan streams executor trace blocks (tier-tinted), verifier emits a "verified"/"issues" block; `Ctrl+C` interrupts the live step.
- `,,` opens the palette; typing `git` fuzzy-filters `git commit`/`kubectl get` + recent blocks; `Enter` dispatches; `Esc`/`Enter` closes.
- `Ctrl+\` (proposed split bindings — §5.2's default-key list defines none yet) (vertical split) / `Ctrl+Shift+\` (horizontal) → divider drags live; focus follows pointer; each split is its own PTY (two `ls` in different cwds).
- `Ctrl+P` theme switch `dark→light→auto` repaints in <1 frame with no grid re-feed.

## 8. Risks / rejected alternatives

- **Rejected: block chrome as a separate GPU panel (editor-style "block list").** Violates §2.1 (terminal must be canonical) and §7 ("only visible blocks in the GPU frame, not scrollback"). Anchoring headers to the grid is the only model that keeps the transcript = real shell output.
- **Rejected: row insertion for block headers.** Desyncs `zet_term::Terminal` cursor/scrollback math and the Recorder's absolute-row OSC 133 markers. Anchored overlays reuse the proven `draw_search`/`draw_ghost` overlay technique.
- **Rejected: a `zet-ui` crate.** §8:139–153 + briefing "no new crates." Would also duplicate `App::pump`/tab state already coherent in `zet`. Chrome state lives in `window/`.
- **Rejected: cosmic-text/harfbuzz dependency for the chrome text.** The existing `slot_for`/`draw_text_row` glyph cache (`renderer.rs:412-447`) already ships mono glyphs with a fallback path. Adding a shaping dep for block headers would cost the §7 idle-RSS budget (~26 MB LLVM/headroom) for no terminal feature.
- **Rejected: fixed 120fps idle loop.** §7 explicitly rejects it ("mutually exclusive with sleeping idle"). A4 keeps the 4 Hz heartbeat and only polls while a tween is active — the exact tradeoff §5.3 + §7 mandate.
- **Risk:** block-header text can exceed its cell segment. Mitigated by `window/blocks.rs` truncation (mirrors `tab_labels` at `mod.rs:852` which already truncates to `seg_w-4`).
- **Risk:** theme `auto` flicker on `ThemeChanged`. Mitigated by `apply_palette` touching only `bg` + re-emitting `quads` on the next demanded frame (no `term.feed`).

## 9. File-to-file impact map (minimal, reviewable)

```
zet-gpu/src/renderer.rs     + Palette, apply_palette, draw_fill, draw_text_row,
                            draw_divider, draw_palette, draw_terminal_in (draw_terminal→wrapper)
zet-gpu/src/lib.rs          re-export: Palette, Rect
|zet-gpu/src/damage.rs       + `Damage::covers(&Rect) -> bool` helper (LoadOp::Clear vs Load) — builds on existing Rect/Damage (damage.rs:4-65)
zet/src/window/mod.rs       + anim import, splits import, palette import, block Views,
                            snapshot block in RedrawRequested, palette keybinding (,,),
                            split keybindings, tab drag-reorder/dup/pin
zet/src/window/theme.rs     NEW  — ThemeDef, BaseScheme, Tokens, resolve() → Palette
zet/src/window/anim.rs      NEW  — Tween, Animated, Ease
zet/src/window/blocks.rs    NEW  — BlockView, BlockListView, agent-trace render
zet/src/window/splits.rs    NEW  — SplitTree, SplitNode, SplitChild, Viewport
zet/src/window/palette.rs   NEW  — CommandPalette, PaletteEntry, PaletteKind, FuzzyIndex
|zet-agent/src/lib.rs        + with_journal_sink(Sender<JournalEntry>), send at every JournalEntry push site (success + denied branches)
zet-suggest/src/ghost.rs    + GhostKind, visible_kind(), preview_kind()  [additive]
ZET-DESIGN.md §11           extend [ui] with theme/auto/tokens (design + config)
```

Every item maps to an **existing** type and an **existing** method; nothing here requires `zet-mcp`, `zet-daemon`, or `zet-context` to grow new APIs (they are *consumed* as-is: `zet_context::gather`/`git_state`, `zet_models::keys::*`, `zet_policy::classify`/`decide`/`Tier`, `zet_pty::Session`/`ShellState`).
