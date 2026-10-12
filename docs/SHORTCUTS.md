# ZET shortcuts & local setup

Everything below describes the built window (`cargo run -p zet-gpu
--example shell`, the desktop launcher target). No account, no login.

## Typing & completion

| Keys | Does |
|---|---|
| `Tab` | Accept the dimmed ghost suffix (history completion). With no ghost, plain Tab reaches the shell (its own completion). Notices and hints are never Tab-typed. |
| `Esc` | Dismiss ghost / cancel an in-flight answer / exit goal mode. Swallowed only when it did something; otherwise passes to the shell. |
| `Enter` | Known commands run. Anything else translates via the model (preview swaps in). In goal mode, starts the turn. While an answer is in flight, Enter waits. |
| `Ctrl+Shift+V`, `Shift+Ins` | Paste (newlines become Enter). |
| `Backspace` | Erase via the shell's own VERASE — works in every line discipline. |
| `Del` | Forward-delete (`ESC[3~`), handled by the shell natively. |
| `Ctrl+C` | Normal SIGINT. While an agent turn runs, the *only* key that reaches the shell: the brake that cancels its running command. |

Ghost text is instant history (`Store::complete`, exit-0, most-recent);
single-row input only, cursor-at-end only, multi-line history truncates
to its first line.

## Talking to ZET (no modes, no chords)

Just type — command or plain words — and press `Enter`:

- A known command (`ls`, `cd`, your aliases) runs instantly. The fast
  path never touches the model: zero added latency.
- Anything else (`hello`, `update`, `list files in detail`) goes to the
  model, and the line swaps to its answer as a preview — review, then
  `Enter` to run. Title never changes; there is no mode to enter or leave.
- `zet` is a verb: `zet update linux` runs the answer, it doesn't
  preview it. Real subcommands (`zet blocks`, `zet inline …`) run
  untouched. Critical (T3) answers still preview with a review notice —
  some orders need eyes first.
- Model failures notice loud (`(model error: rate limited: …)`) — free
  tiers throttle; wait a minute and retry.
- Needs a model, resolved per request: `ZET_INLINE_SPEC` first, then the
  `zet inline <spec>` choice, then the window's startup default, then a
  fresh local probe. Set it from inside any window with e.g.
  `zet inline openrouter:openai/gpt-4o-mini` (or `zet inline off`) —
  windows pick it up with no restart. `zet inline` shows the current one.
  `zet models` lists choosable models as ready specs (OpenRouter key,
  local server, or a compat URL; `--free` keeps free-tier ids).
  With nothing configured: sticky `(no model: …)` hint, nothing runs.
- Broken replies never swap: `(invalid: …)` / `(no command)` notice instead.
- Greetings and smalltalk get an `agent: …` reply on the prompt line
  instead of a command — `hello` gets answered, not exit 127.
- If the model fails, your original line runs untouched — ZET never
  blocks your input; worst case is the shell's own error.
- Two honest limits: new installs mid-window aren't in the command set
  until restart (their first use translates to a preview — safe, just
  surprising once); multi-line pastes translate as one joined line, so
  review the preview before running.

## Agent mode (`Ctrl+G`)

Describe a goal, `Enter`; the agent plans and runs steps, each typed
**visibly** into the shell (normal recorded blocks with exit codes).
Title shows `(agent)` then `(agent…)` while busy.

- Entering agent mode pre-consents T0–T2; T3 never runs (model sees a denial).
- Input locks during a turn (only `Ctrl+C` gets through).
- Every step journals (`zet journal`); repos checkpoint before mutations.
- Done notice: sticky `agent: …` summary on the fresh prompt.

## Undo (`F12`)

Restores the last checkpointed agent step (`reset --hard` + untracked
clean + stash pop scoped to the shell's cwd). `(nothing to undo)` when the journal is
empty; `(undo failed: …)` otherwise. Wrong repo fails closed. Swallowed
while a turn runs.

## Window chrome

- Title carries `(agent)` / `(agent…)` while a turn runs, plus the last
  `(exit N)`. There are no input modes left to flag. `exit` (or `Ctrl+D`)
  closes the window.
- Every command is recorded: `zet blocks` lists, `zet journal` shows
  agent turns. The store scrubs known key material (`sk-*`, `ghp_*`,
  `AKIA…`, `Bearer …`); anything else pasted persists — rotate it if
  exposed. `auth add` reads with echo off so keys never hit the grid.

## Local model setup (no sudo, ~2GB disk)

```bash
# one time: user-space ollama (no sudo) + a small model + persistent service
mkdir -p ~/.local/share/ollama-pkg ~/.local/bin
curl -fsSL -o /tmp/ollama.tgz \
  https://github.com/ollama/ollama/releases/latest/download/ollama-linux-amd64.tgz
# (ARM: ollama-linux-arm64.tgz)
tar xzf /tmp/ollama.tgz -C ~/.local/share/ollama-pkg && rm /tmp/ollama.tgz
cp ~/.local/share/ollama-pkg/bin/ollama ~/.local/bin/
# ~/.config/systemd/user/ollama.service:
#   ExecStart=%h/.local/bin/ollama serve
#   Environment=OLLAMA_MODELS=%h/.local/share/ollama-models
systemctl --user enable --now ollama
ollama pull llama3.2:1b
```

With that running, NL and agent turns work with zero config
(autodetected `local:llama3.2:1b`; `ZET_INLINE_SPEC` overrides).
`qwen3:1.7b` reasons (~100s on CPU) — quality runs only.
