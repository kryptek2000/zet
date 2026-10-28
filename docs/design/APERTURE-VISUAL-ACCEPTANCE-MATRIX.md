# ZET Aperture Instrument — Native Visual Acceptance Matrix

**Status:** implementation contract for `feat/aperture-flagship`  
**Reference:** `/home/bandit/.codex/generated_images/01a1847f-dc23-7572-b6a1-a0ace4cf7cb7/exec-e7eed55e-6df1-4350-b204-0a001e894250.png`  
**Reference SHA-256:** `7d1886e7cc70ee24ef7f0ce8e25bdda7e8fa43d654951d84883938a1edc5e020`  
**Reference dimensions:** 1672 × 941 PNG  

This is a native implementation target, not a claim that the reference is a
runtime screenshot. Illustrative commands, times, branches, counts, and the
reference watermark are not runtime data and must not be copied into ZET.

## Acceptance matrix

| Reference area | Native counterpart | Fidelity requirement | Deviation / reason |
|---|---|---|---|
| ZET logo and upper chrome | `draw_app_chrome` plus renderer aperture-mark primitive | A large geometric cyan ZET mark, confident wordmark, dark header, and live session identity establish product ownership at first glance. | System decoration remains outside the app; only supported native chrome is drawn. |
| Main proportions | `layout::resolve` with semantic rail/terminal/Inspector widths | Large workspace targets approximately 18% rail / 56% terminal / 26% Inspector while retaining `MIN_TERM_COLS`. | Cell quantization and narrow-window refusal can move boundaries by a few columns. |
| Mission Rail structure | `draw_rail` and `panels::rail_rows` | Rail reads as an indexed instrument: `01 WORKSPACE`, `02 SESSIONS`, `03 COMMAND LEDGER`, `04 RESOURCES`; navigation is visibly separate from evidence detail. | Resources stay honest and noninteractive when no resource source exists. |
| Indexed navigation spine | Rail pixel seam and section markers | A thin vertical structural line, compact indices, and controlled cyan active seam replace a plain text column. | Spine is pixel geometry around cell-aligned text so PTY geometry is unchanged. |
| Session selection | Rail session rows and existing `rail_sel` focus | Active session is immediately legible through surface, seam, marker, and type—not color alone; real cwd/state/exit remain the only data. | No invented session names or telemetry. |
| Command rows | Observed rows in `draw_rail` | Command ledger uses disciplined row alignment, duration/exit reservation, failure markers, and stable selected-command linkage. | Long rows use existing Unicode-safe truncation. |
| Living Terminal frame | `draw_terminal_in` plus terminal-frame chrome | The terminal remains the largest, calmest, near-black surface with a precise cyan edge seam and integrated `LIVING TERMINAL` header. | No decorative pixels are drawn over PTY cells or output. |
| Inspector header | Inspector composition in `RedrawRequested` | Header identifies `CONTEXTUAL INSPECTOR`, uses the same aperture seam, and visually belongs to the workspace rather than a generic card. | Header text remains cell-rendered for font fallback and Unicode correctness. |
| Selected command module | Contextual selected-command surface | Selected command is the dominant Inspector object with real command text, observed status, exit, duration, cwd, branch, and session context. | Unavailable fields remain `—`; no timestamps are inferred. |
| Evidence detail hierarchy | Inspector sections: `SELECTED COMMAND`, `OBSERVED RESULT`, `RECORDED OUTPUT`, `PROVENANCE` | Technical facts are grouped, labeled, and separated by hierarchy; the Inspector explains the selected item instead of mirroring the rail. | Existing keyboard row contract remains intact while the visual hierarchy is reordered. |
| Output preview container | Bounded sanitized output well | Captured output has a distinct inset surface, readable monospace text, safe clipping, and explicit empty/unavailable/truncated state. | Preview remains bounded by the P3-A1 recorder contract. |
| Provenance section | Inspector footer/details | Session identity and recorded-command origin are visible as observed context without implying verification, files, CI, or causality. | No provenance is rendered when the data is unavailable. |
| Color balance | `zet_gpu::theme::APERTURE_INSTRUMENT` semantic roles | Near-black graphite dominates; cyan/violet/teal/red/amber are sparse semantic accents, not a blue-gray wash. | ANSI terminal colors continue to follow terminal semantics. |
| Typography | Existing cosmic-text/glyph atlas with role-specific sizes via geometry and spacing | Wordmark, uppercase section labels, command text, metadata, and output have distinct hierarchy and generous scanning rhythm. | The current atlas remains the fallback-safe native text path; no external font dependency is introduced. |
| Depth and accents | Chamfered panel, seam, inset, and selected-state primitives | Selective architectural cut corners, layered inset surfaces, 1–2 px seams, and restrained elevation produce Aperture's instrument depth. | No full-screen shader, continuous glow, or expensive animation is introduced. |

## Semantic design tokens

The default Aperture theme uses the approved graphite/cyan anchors:

| Role | Token |
|---|---|
| space | `#080B13` |
| terminal | `#101522` |
| elevated graphite | `#141C2B` |
| deep inset | `#0B111D` |
| precision border | `#27354B` |
| primary text | `#E8F1FF` |
| secondary text | `#9AAFC6` |
| muted text | `#7188A1` (lifted from the reference anchor for compact-text contrast) |
| electric cyan | `#49DFFF` |
| contextual violet | `#AA8AFF` |
| precision teal | `#46D4BD` |
| failure red | `#FF667B` |
| warning amber | `#F0B96A` |

The tokens describe semantic meaning, so a future user theme can replace the
palette without changing panel or evidence semantics.

## Native proof status

The source image is the approved visual reference. Final side-by-side native
capture, narrow-window capture, and owner visual acceptance remain preview
steps because desktop capture must be owner-supervised. The implementation
must report material deviations rather than label the result pixel-perfect.
