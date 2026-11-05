# P2-B — native ASCII identity evidence

These are real native captures of the ZET window from the P2-B repair build on
2026-11-05. They were taken with `grim` against the live Wayland application;
no mockups, composited stills, or synthetic terminals were used.

| File | State |
|---|---|
| `01-welcome-prompt.png` | Normal shell prompt visible at the top of the terminal while the full approved ZET wordmark, brand copy, tagline, and supporting identity remain centred in genuinely unused rows. |
| `02-after-interaction.png` | The same terminal immediately after typing `echo zet`. The identity is gone before the first character reaches the PTY; the typed command and cursor are unobstructed. |
| `03-narrow-window.png` | A 300×420 logical-pixel window. The full wordmark cannot fit, so the one-line `ZET // APERTURE` identity is shown without clipping while the live prompt stays visible. |
| `04-fullscreen-tui.png` | OpenCode running full-screen inside ZET. No ZET artwork is present in the alternate screen or over the TUI. |

## Reproduction

```bash
cargo build --release -p zet
./target/release/zet
```

Disable the identity for a session with:

```bash
ZET_NO_WELCOME=1 ./target/release/zet
```

The screenshots show the production rendering path: the identity is planned
from actual terminal-cell occupancy and emitted through the native pixel UI
text stream after the terminal frame. The PTY grid, cursor, scrollback, and
shell history are never written or repainted by the identity.
