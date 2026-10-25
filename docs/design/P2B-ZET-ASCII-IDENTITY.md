# P2-B — ZET Signature ASCII Identity

**Status:** Owner-approved planning direction, 2026-10-25. Planning only; not implemented.
**Tracking:** [Issue #29](https://github.com/kryptek2000/zet/issues/29)
**Roadmap:** [P2-B](../ROADMAP.md)
**Flagship direction:** [ZET Flagship UI Reference](ZET-FLAGSHIP-UI-REFERENCE.md)

This note records the addition of a ZET ASCII wordmark to the approved P2-B
flagship visual plan. It does not approve final artwork, typography, exact
placement, or implementation. The recent owner-approved ASCII concept is a
visual reference, not an implementation specification; no image asset is
claimed to be committed to this repository.

The wordmark is a branding layer inside the existing direction:
**Mission Rail | Living Terminal Workstream | Contextual Inspector**. It is not
a competing layout, a replacement for the real terminal, or a dashboard.

## Design direction

The official design direction includes a distinctive, large, monospace
ASCII-style **ZET** wordmark with these characteristics:

- Clearly reads **ZET**.
- Futuristic, technical, and terminal-native.
- Sharp, recognizable letterforms.
- Strong silhouette even without color.
- Compatible with selectable ZET themes.
- Emerald-green/cyan treatment may be explored without replacing the existing
  approved theme palette.
- Scales gracefully across terminal sizes.
- No excessive glow or distracting animation.

## Intended usage

**Primary**

- The native welcome / startup experience.
- An empty or introductory workspace state, without replacing the real shell.

**Secondary possibilities**

- A ZET About screen.
- Version and diagnostic command output that is explicitly ZET's own output.
- An optional branded terminal information display.

## Hard constraints

- Never inject fake commands, output, sessions, or system facts into the real
  PTY.
- Never overwrite shell output or user content.
- Never obstruct shell input, scrollback, selection, or full-screen terminal
  applications.
- Never fabricate system, model, CI, or telemetry information.
- The wordmark must be optional or dismissible wherever it occupies workspace.
- The existing full-featured terminal must remain usable with no branding
  screen and with branding disabled.
- Preserve startup performance and accessibility; do not delay the first prompt
  or block the UI thread.
- Define narrow-window behavior: reduce or omit the art rather than clipping it
  into terminal content.
- Define screen-reader behavior: provide a textual label or skip path; do not
  make essential controls depend on decorative-only content.

## Future implementation acceptance criteria

A future native implementation must demonstrate:

1. A recognizable ZET ASCII wordmark in the native application.
2. Correct rendering without terminal-content corruption.
3. Theme-compatible colors and adequate contrast.
4. Graceful handling of narrow windows.
5. No synthetic PTY history or fabricated system data.
6. No meaningful startup or rendering performance regression.
7. Owner visual approval against the chosen reference.

## Open design questions

- Final ASCII letterforms: one fixed grid or responsive variants?
- Exact monospace font stack and fallback behavior for missing block or
  box-drawing glyphs; pure ASCII-7 versus terminal-safe Unicode.
- Startup placement: overlay, header, empty-state panel, or a combination, and
  the default/dismissal policy.
- Theme mapping and contrast floor for every selectable theme.
- Screen-reader label and whether the art is exposed as text, an image role, or
  skipped.
- Narrow-window breakpoints and the minimum usable wordmark size.
- Whether version/diagnostic output includes the wordmark and how it degrades
  in pipes or non-TTY output.
- Whether emerald/cyan is a default treatment or an optional theme accent.
- Interaction with reduced motion and the existing startup performance budgets.

## Approval boundary

Exact ASCII artwork and typography require a separate design approval before
production implementation. This note is a planning and visual-acceptance
contract only; it does not authorize a Rust/UI implementation slice.
