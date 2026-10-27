# P2-B1 — Terminal Compatibility Smoke Test

**Date:** 2026-10-25
**Environment:** nested Hyprland 0.56.2, real bash PTY, debug build.
**Before binary:** `origin/master` at `d9bca1b`.
**After binary:** `work/p2b1-native-ui-slice` at `0e42b51`.

The P2-B1 changes are confined to chrome drawing and rail data formatting. The
terminal parser, PTY reader, input translation, scrollback, selection and
clipboard paths are not modified. The full workspace suite passes; this note
records the manual application smoke test and one **pre-existing** limitation.

## tmux — works

`tmux 3.8` starts in the real PTY. The screenshot
[`07-compat-tmux.png`](07-compat-tmux.png) shows:

- the tmux status bar rendered in the terminal grid;
- the rail session state changing to `running` with the real green `●` dot;
- the real recorded command row `› tmux 0 31591ms` after the client exited.

This is the real tool, not a mock. ZET's rail observes the shell running state
truthfully.

## Neovim — pre-existing DSR limitation, not a P2-B1 regression

`NVIM v0.12.6` with `-u NONE` reports:

```
E1568: Terminal did not respond to DSR request for 'background' color.
Startup may be slower. :help 'ttyfast'
```

and leaves the screen partially rendered. The same failure occurs with the
**master** binary in the same environment:

- after branch: [`08-compat-nvim-after.png`](08-compat-nvim-after.png)
- master: [`09-compat-nvim-master.png`](09-compat-nvim-master.png)

The cause is a missing terminal-response path (DSR / OSC 10/11 color query) in
`zet-term`, not the rail/tab/chrome drawing changed here. It is a genuine
terminal-compatibility gap that predates P2-B1 and is **not** fixed or hidden by
this slice.

## Consequence

- No claim is made that Neovim is fully compatible in this baseline.
- The P2-B1 UI change did not introduce the failure; the master capture proves
  it is pre-existing.
- A separate terminal-compatibility slice should implement the required DSR /
  color-query responses, then re-run Neovim, tmux, alt-screen and scrollback
  tests before the project advertises editor compatibility.

The UI slice remains safe for real PTY use: tmux, shell input/output, tab
switching, rail focus, resize and the existing test suite are unaffected.
