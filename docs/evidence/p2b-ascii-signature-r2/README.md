# P2-B-R2 — signature identity premium polish evidence

Real native Wayland captures of ZET, taken 2026-11-05 with `grim` against the
live application. No mockups, synthetic terminals, or composited fake output
were used. The display was running at a 1.5× fractional scale, so the 1256×670
logical window appears as 1884×1005 physical pixels.

| File | State |
|---|---|
| `01-before-standard.png` | R1 before-state, copied unchanged from `docs/evidence/p2b-ascii-signature/01-welcome-prompt.png` for the comparison. |
| `02-after-standard.png` | R2 premium identity at the same 1256×670 logical window size, with the real shell prompt visible. |
| `03-before-closeup.png` | 2× nearest-neighbour close-up of the R1 wordmark and supporting type. |
| `04-after-closeup.png` | 2× nearest-neighbour close-up of the R2 wordmark and supporting type. |
| `05-before-after-comparison.png` | Equal-size before/after composite of the two native standard captures. |
| `06-large-fullscreen.png` | Fullscreen 1280×720 logical / 1920×1080 physical composition. |
| `07-narrow-window.png` | 300×420 logical narrow window with the minimal `ZET // APERTURE` mark. |
| `08-after-dismissal.png` | Standard window immediately after typing `echo zet`; identity gone, prompt and cursor unobstructed. |
| `09-fullscreen-tui.png` | OpenCode full-screen inside ZET; no identity artwork over the TUI. |
| `10-compact-window.png` | 1000×215 logical compact composition: wordmark plus brand line, no clipped copy. |

## Rendering refinement

The R1 thin outlines were the approved art's double-line box-drawing
characters (`╔ ╚ ╗ ╝ ═ ║`). JetBrains Mono Nerd Font draws those strokes inset
and centred inside their own cells, while `█` fills the full cell. Mixing them
creates the offset doubles that wandered around the block edges; the UI text
stream added cell-advance and subpixel variation on top. A terminal-cell render
of the same art showed the same offset, proving it was not the R1 UI pass.

R2 keeps the approved 48×8 character grid as the source of truth but renders
its solid silhouette through pixel-snapped native geometry: exact full cells
for `█`, exact lower halves for `▄`, and the horizontal box runs become two
precise cyan rules attached to the row's top edge with one shared
thickness/gap pair. The vertical `║` accents are intentionally omitted because
the solid stem already carries the silhouette; this removes the floating
one-sided outlines. The letterform silhouette and the approved copy strings are
unchanged, and the whole wordmark is pixel-centred in the viewport.

## Typography

- Brand: `ZET // APERTURE INSTRUMENT` — near-white, 0.72× cell height, weight
  700, wider tracking.
- Tagline: `THE COMMAND LINE, REIMAGINED.` — cyan accent, 0.62× cell height,
  weight 500, restrained tracking.
- Supporting: `NATIVE TERMINAL / APERTURE` — secondary grey, 0.54× cell
  height, still legible.
- All three share one baseline grid inside their rows.

## Capture notes

The R1 comparison image is the existing committed capture; it has not been
overwritten. R2 images use the same standard window dimensions for direct
comparison. The TUI capture is the real OpenCode interface running inside ZET.
