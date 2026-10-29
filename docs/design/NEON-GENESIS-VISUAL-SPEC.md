# ZET — Neon Genesis Native Visual Specification

**Status:** implementation specification for `feat/neon-genesis-flagship`
**Source:** P3-A1 candidate `bf56f7f7fb478f45bfe74db06741ca569a8a71ce`
**Scope:** native presentation architecture and visual system; no new mission, agent, CI, approval, or verification data

## Direction

Neon Genesis gives ZET a recognizable instrument-like identity: deep-space surfaces, precise cyan edge signals, restrained violet hierarchy, and dense but calm information architecture. The terminal remains the largest and most legible surface. The rail and Inspector frame observed work; they never replace shell output or imply an outcome that the application did not observe.

The redesign uses two coordinate systems together. Terminal content remains cell aligned so PTY geometry, selection, Unicode behavior, and resize invariants are preserved. Panel chrome uses bounded pixel geometry for inset surfaces, rounded corners, accent rails, and status markers, then returns to the existing glyph atlas for text. No webview, browser UI, continuous animation, or full-screen effect is introduced.

## Semantic tokens

The default theme is expressed through `zet-gpu::theme::NEON_GENESIS`; renderer and composition code consume semantic roles rather than literal colors.

| Role | Default | Use |
|---|---|---|
| `space` | `#080B13` | window clear and focus-mode foundation |
| `terminal` | `#101522` | live PTY canvas |
| `surface` | `#0D1422` | rail and Inspector base |
| `surface_raised` | `#141C2B` | cards and active surfaces |
| `surface_inset` | `#0A101C` | output wells and metadata fields |
| `border` | `#27354B` | quiet separation |
| `border_strong` | `#34506C` | focused separation |
| `text` | `#E8F1FF` | primary labels and commands |
| `secondary` | `#9AAFC6` | metadata and supporting copy |
| `muted` | `#7188A1` | unavailable or tertiary information; lifted slightly for compact metadata contrast |
| `cyan` | `#49DFFF` | identity, focus, active work, observed links |
| `violet` | `#AA8AFF` | selected evidence and elevated context |
| `teal` | `#46D4BD` | observed healthy state |
| `red` | `#FF667B` | observed nonzero exit or dead session |
| `amber` | `#F0B96A` | warning and bounded/truncated state |

Theme fallback remains deterministic and can be replaced by a future user theme without changing component semantics. Teal and cyan indicate observed interface state only; they do not certify a mission or fix.

## Composition

When either panel is visible, row zero becomes the integrated ZET chrome. It contains the ZET mark and current layout on the left, real tab/session identity in the center, and restrained focus state on the active segment. Terminal Focus with no panels keeps a full-width terminal and does not force dashboard chrome.

The resolved layout remains `Mission Rail | Living Terminal | Contextual Inspector`. The existing pure resolver owns viewport allocation and the terminal keeps its minimum width. The Inspector collapses before the rail, and both collapse before the terminal is reduced below `MIN_TERM_COLS`.

### Mission Rail

The rail is a command surface, not a text dump. Its real sessions render as two-row cards with a selected surface, a narrow identity rail, a semantic state marker, a cwd tail, and a clear active/selected distinction. The activity stream uses compact rows with a failure marker, reserved exit/duration fields, and a shared selected-evidence treatment that can also appear in the Inspector.

The rail header identifies the surface and current layout. Empty mission support remains an honest future extension point; no mission, progress, agent, or approval rows are rendered until a real source exists. The footer contains only the observed cwd.

### Living Terminal

The PTY grid remains the hero. Its framing comes from adjacent surfaces, consistent insets, focus separation, and the existing cursor/selection behavior. No overlay is injected into PTY output. Focus mode removes panel chrome and returns the complete terminal viewport, sessions, scrollback, and shell input.

### Contextual Inspector

The Inspector is organized around the question “what am I looking at, and what actually happened?” Its header and session overview use real tab, cwd, shell phase, and last observed exit data. Recent commands remain available as a bounded selection index, but the primary detail surface is a selected command card:

- command text with observed identity;
- exit code and measured duration;
- recorded cwd and Git branch when present;
- bounded, sanitized captured output;
- explicit `OUTPUT UNAVAILABLE`, `EMPTY OUTPUT`, or `PREVIEW TRUNCATED` states.

The Inspector never renders file attribution, CI, approval, verification, agent, or mission-completion claims. Selection remains session scoped and follows stable block identity.

## Interaction states

Hover is quiet and local. Focus adds a visible edge or accent bar. Selection changes surface, edge, and text weight together so color is not the only signal. Failed commands use a red marker and danger surface; unknown values use muted text and an explicit em dash. Disabled or unavailable states remain visually non-actionable. Keyboard focus stays first class: panel typing never reaches the PTY, Escape returns to the terminal, and existing shortcuts remain intact.

## Responsive and performance rules

Panel geometry is derived once from the resolved frame and all hit testing uses the same viewports. At narrow widths, secondary surfaces collapse before terminal width is compromised. Pixel primitives are clipped, bounded, and batched into the existing quad stream. There are no timers, animation loops, extra threads, full-screen shaders, or per-frame database/Git queries. Snapshot caches continue to refresh only on observable state changes or the existing heartbeat.

## P3-A1 preservation

The visual layer consumes the existing `InspectorSnapshot`, `InspectorCache`, `CommandRow`, stable block IDs, bounded output preview, secret scrubbing, and session-scoped recorder. It does not change PTY input, terminal grid mutation, scrollback, shell integration, command recording, output limits, or provenance semantics. The implementation is accepted only when the existing workspace checks and the targeted layout, geometry, selection, and Inspector tests remain green.
