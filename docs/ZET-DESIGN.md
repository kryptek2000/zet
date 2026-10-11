# ZET — Product + Architecture Spec
**Version:** 0.1.0-draft — 2026-10-09
**Stack:** Rust + GPU (wgpu) | **Edge:** Agentic AI that runs tasks
**Goal:** Beat Ghostty on speed, beat Warp on agents, keep terminal trust.

---

## 1. Positioning

> ZET is the terminal that does the work, not just the commands.

- **Ghostty** = fastest dumb pipe. **Warp** = smart but heavy, closed, account-walled.
- **ZET wedge:** Ghostty-class speed + a local-first agent runtime that plans, runs, verifies, and rolls back — in your real shell, over SSH, with any model, with undo.
- One-line test: `zet "upgrade this repo to Tailwind v4 and open a PR"` should produce a plan, run it block-by-block, show diffs, and let you undo with one key.

## 2. Principles (non-negotiable)

1. **Terminal first, chat second.** The shell stays canonical. AI renders as blocks, never hijacks the PTY.
2. **Speed is a feature.** Budgets in §8 are release gates, not aspirations.
3. **Agent is accountable.** Every agent action = visible block + diff + audit entry + undo.
4. **Local-first, BYOM.** Offline autocomplete works. Cloud models are pluggable. No account required to open a shell.
5. **Ask rarely, but at the right time.** Tiered approvals (§6.4), not Y/N spam.
6. **Keyboard everything.** Agent, palette, blocks, diffs — all reachable without a mouse.
7. **Standards over lock-in.** xterm + shell integration via OSC, MCP for tools, Markdown for blocks, TOML for config.
8. **No silent writes.** Agent never mutates without a recorded checkpoint.

## 3. Who it's for

- **P1 — Senior dev / DevOps:** lives in terminal, wants delegatable chores (upgrades, triage, k8s, incident logs).
- **P2 — Full-stack builder:** wants repo-aware coding agent without leaving terminal.
- **P3 — Team lead:** wants shareable runbooks + repeatable agent recipes.

Jobs: fix failing CI, upgrade deps, write + run migration, triage `kubectl` incident, scaffold + PR a feature, explain this error and fix it.

## 4. Competitive teardown (what we steal / avoid)

| Tool | Strength to match | Weakness to exploit |
|---|---|---|
| Ghostty 1.x (Zig, Metal/GL, 120fps, ~2ms, 45MB, 0.08s cold) | perf bar, native feel, no telemetry | no AI, no blocks |
| Warp Agent (real-terminal agent, parallel/cloud agents, MCP, approvals, third-party CLIs) | agent-in-real-terminal, delegated/cloud, Factory MCP | heavy (~280MB), account required, closed, 8ms latency |
| Amazon Q / Fig | 100s of CLI completions, AWS depth | shallow outside AWS, overlay not runtime |
| Alacritty/WezTerm/kitty | GPU, standards, config | no agent story |
| Cursor/Zed terminal | codebase context | editor-locked |

**ZET must:** match Ghostty perf (§8), match Warp's real-terminal agent + parallel + delegated, and win on: startup with no login, memory <100MB, local model path, open MCP runtime, one-key undo, SSH-resilient agents.

## 5. UX System

### 5.1 Three modes (explicit, switchable with `Ctrl+.`)

1. **Inline** (default shell): ghost-text autocomplete (local 1B model, <50ms), `Tab` accept, `Ctrl+;` natural-language→command preview.
2. **Block** (Warp-style but faster): each command = block with cwd/git/exit/duration, copy/fork/share, `Cmd/Ctrl+P` palette over blocks.
3. **Agent** (right-pane or full-takeover): goal → plan → run → verify. Streams tool calls as collapsible blocks. `Esc` pauses, steering text re-plans mid-run.

Plus **Delegated**: `zet run --bg "nightly e2e + report"` → detaches to local daemon or SSH host, notifies on done/approval-needed. Same view locally or over SSH.

### 5.2 Key surfaces

- **Prompt:** minimal, shows agent status dot (idle/working/needs-approval), git branch, last exit. Never blocks typing.
- **Diff review:** agent edits batch into one review (like `git add -p` on steroids): accept/hold/edit per hunk, then apply.
- **Timeline:** every session = replayable log (`zet replay <session>`). Audit + onboarding + incident postmortem.
- **Palette:** fuzzy over commands, blocks, agent recipes, MCP tools, settings.
- **Notifications:** approval/finish/fail — OS notification + terminal bell + `zet status`.

Default keys: `Ctrl+.` mode, `Ctrl+;` NL-command, `Ctrl+Shift+R` agent review, `u` undo last agent checkpoint, `/` recipes.

### 5.3 Visual identity

Dark-first, JetBrains Mono default, 24-bit + ligatures + Nerd Fonts, opacity + blur optional. Blocks are flat, 6px radius, 1px border — density over decoration. 120Hz motion only for cursor/scroll; AI streaming never janks scroll.

## 6. Agentic Core (the differentiator)

### 6.1 Loop: Propose → Plan → Execute → Verify → Report

```
goal → ROUTER (local classify: direct cmd vs plan vs recipe)
     → PLANNER (model + context pack → steps with risk tags)
     → APPROVAL GATE (policy §6.4)
     → EXECUTOR (runs step as PTY block, streams, captures exit/output)
     → VERIFIER (exit code + assertions + optional `zet check` script)
     → on fail: re-plan (max N) or pause for user
     → CHECKPOINT per mutating step → REPORT (what ran, diffs, undo cmd)
```

Max parallelism: 1 mutating lane + N read-only lanes. Multi-agent: parent spawns children per worktree; Agent Panel lists all with pause/kill/steal-output.

### 6.2 Tools (all via MCP internally)

Built-in: `shell.exec` (PTY-backed), `file.read/write/patch`, `git.*`, `gh.pr`, `docker`, `kubectl` (read-gated), `web.fetch`, `code.search` (ripgrep + AST). External: any MCP server from `zet.mcp.toml`. Third-party CLIs (Claude Code, Codex, OpenCode) run as first-class panes with shared context + ZET review UI — same play Warp uses, but without forcing our agent.

### 6.3 Context pack (auto, capped, cited)

Every agent turn gets: cwd, git branch/status/staged diff (truncated), shell history (last 50), env allowlist, running procs, selected block, open recipe/rules, `AGENTS.md`/repo map (ctags/LSP symbols, 8k token budget). UI shows token count + what was included. Rules precedence: project `zet.rules.md` > global > model default.

### 6.4 Safety: tiered approvals + checkpoints

| Tier | Examples | Default |
|---|---|---|
| T0 read | cat, ls, git status, kubectl get | auto |
| T1 low-write | file edit in repo, npm install, git commit | auto in repo, ask outside |
| T2 risky | git push, docker run, db migrate, `rm -rf` scope, curl\|sh | always ask + show exact cmd + diff |
| T3 critical | prod kubectl, secrets access, sudo, mass delete | ask + type-to-confirm + sandbox only |

- **Checkpoints:** pre-write snapshot (git stash-like overlay + file journal). `u` or `zet undo` reverts last checkpoint. All agent writes journaled to `~/.zet/journal/<session>.sqlite`.
- **Sandbox:** Linux: bubblewrap + Landlock; macOS: Seatbelt profile; least-privilege net/fs. Prod contexts forced to dry-run plan first.
- **Redaction:** secrets (env, `~/.aws`, `~/.ssh`) never sent to cloud models unless explicitly attached; local redactor with deny-list + entropy scan.

### 6.5 Recipes (repeatability beats chat)

`~/.zet/recipes/*.md` + repo `.zet/recipes/*.md`: named parameterized plans (`upgrade-tailwind`, `triage-ci`, `k8s-crashloop`). Versioned, shareable, runnable headless: `zet run recipe:triage-ci --arg service=api`. This is the team moat Warp calls Drive/Runbooks.

## 7. Performance Spec (release gates)

Measured 2026-10-11, release binary, Intel iGPU (Vulkan), X11
(ZET_PERF=1 stage marks + smaps, 3-run steady state; first-ever run
excluded — Mesa/font caches cold add ~1s once). Raw logs per run on
request; method: smoke-driven window, screenshot + stderr + ps/smaps.

| Metric | Gate | Measured | Notes |
|---|---|---|---|
| Cold startup (exec→first prompt frame) | <300ms steady | 186–249ms | 60 proc/winit + 64 Vulkan ICD enum + 29 device + 34 GL objs/fonts + ~45 shell/prompt |
| Input latency (key→pixel, warm) | <3ms steady-state | median ~1.0ms, 12/14 samples under gate | ZET_PERF key-to-render; outliers: 8.71 first-key glyph raster, 3.52 unexplained single; under-counts OS wakeup |
| Frame pacing | event-to-photon <8ms, no fixed fps | ~1ms (same ZET_PERF series) | demand-driven (4Hz idle heartbeat); 120fps fixed is mutually exclusive with sleeping idle — dropped deliberately |
| Idle RSS | <100MB | 86–92MB | top-5 64MB (26 LLVM + 13 heap/fontdb + 9 binary + 8 client SHM bufs + 8 unused Vulkan ICDs resident after enum); +22–28MB long tail of sub-MB shared libs |
| `cat 100k lines` | <0.9s | 192ms | Enter-to-recorded-block (589KB output), release, X11 |
| Inline suggest | <50ms history | instant (in-test) | model path needs local 1B runner (open) |
| Agent first-step TTFT | <4s cloud | 1.5–3.6s live | OpenRouter gpt-4o-mini, measured in live tests |

Deliberately NOT cut for M0 (measured, rejected): Vulkan ICD restriction
(portability: VMs need GL fallback; saves ~30–50ms only on multi-ICD
boxes — try `VK_ICD_FILENAMES` locally), system-font scoping (needs
fontconfig integration for ~5–10MB / ~15ms), fixed-120fps (burns idle).
Revisit when a gate trends +20% over two releases.

Perf rules: renderer on dedicated thread, PTY parse off UI thread, no JS/Electron in hot path, wgpu glyph atlas, incremental block virtualization (only visible blocks in DOM/GPU).

## 8. Architecture (Rust + wgpu)

Crates (workspace `zet/`):

```
zet           — binary, CLI (`zet`, `zet run`, `zet replay`, `zet undo`)
zet-term      — VT parser, grid, scrollback, shell-integration (OSC 133/633)
zet-pty       — portable PTY (fork/exec, Win ConPTY), SSH transport
zet-gpu       — wgpu renderer, glyph atlas (cosmic-text/parley), damage tracking
zet-blocks    — block store (SQLite + Arrow IPC for scrollback)
zet-context   — repo map, history, env, git watcher
zet-agent     — planner/executor/verifier, lane scheduler, checkpoints
zet-policy    — tiers, approvals, sandbox profiles, audit log
zet-models    — LLM gateway trait: LocalLlama, OpenAI-compat, Anthropic, BYOK
zet-mcp       — MCP client/host, tool registry, third-party CLI panes
zet-daemon    — background/delegated runs, notifications, `zet status` IPC
```

Threading: UI/GPU thread ↔ PTY reader ↔ agent tokio runtime ↔ model streaming. All cross via lock-free rings; PTY output never blocks on model I/O.

State: `~/.zet/sessions/<id>/` (blocks.sqlite, journal.sqlite, transcript.md). Config: `~/.zet/config.toml` + `zet.mcp.toml` + per-repo `.zet/`.

Model gateway: single `chat_stream(messages, tools)` trait. Router picks: local-small (autocomplete/classify) → user-default frontier (agent) → local-8B fallback offline. Keys per provider in OS keychain, never in files. Zero-retention flag propagates to providers that support it.

BYOK providers (all user-supplied keys, mix-and-match per task):
- **Native:** Anthropic, OpenAI.
- **OpenAI-compatible (`base_url` + key):** OpenRouter, Together, Groq, DeepSeek, xAI, Fireworks, any self-hosted OpenAI-compat endpoint — one config line each.
- **opencode-style:** provider/model IDs resolve through the same gateway, so any model opencode-go supports (incl. via OpenRouter) works in ZET with the same key.
- **Local:** llama.cpp / Ollama / LM Studio (no key, offline-capable; default for inline suggest + classifier).

Rendering pipeline: VT grid → shaped glyph run cache → wgpu atlas → damage rects → present. Sixel/Kitty/iTerm images decoded off-thread, cached as textures. Ligatures via shaping; emoji/Nerd fallback chain.

SSH story: `zet ssh host` ships `zet-daemon` + context pack remotely; agent runs near the code, UI streams blocks (mosh-like reconnect).

## 9. Compatibility

Shells: bash/zsh/fish + nushell (OSC 133 semaphores, cwd + exit + prompt markers). Multiplexers: works inside tmux/zellij (passthrough) and replaces them for most (splits/tabs native). Fonts/images: Nerd, ligatures, Sixel + Kitty + iTerm inline. Platforms: macOS (Metal via wgpu) + Linux (Vulkan/GL) first; Windows ConPTY second.

## 10. Privacy & Compliance

No account to launch. Telemetry opt-in only, crash reports redacted. Team tier: SSO, RBAC for recipes/approvals, central usage + audit export, SOC2 path (follow Warp's lead: zero-retention option, BYOLLM so code stays in-customer). `zet audit <session>` prints every command, diff, approval.

## 11. Config & Extensibility

```toml
# ~/.zet/config.toml
[ui]
font = "JetBrainsMono Nerd Font"; font_size = 13; opacity = 0.96
[shell]
default = "zsh"; scrollback = 500_000
[models]
default = "anthropic:claude-sonnet"; inline = "local:qwen3-1.7b-q4"
# BYOK examples (keys live in OS keychain via `zet auth add <provider>`):
# agent_alt = "openrouter:anthropic/claude-sonnet"
# cheap = "openrouter:deepseek/deepseek-chat"
# custom = "compat:https://my-gateway.internal/v1:my-model"
[agent]
approvals = "tiered"; max_replans = 3; sandbox = true
```

Plugins: WASM or Lua for UI; everything agentic goes through MCP (one interface for built-ins and user servers). Keybindings fully remappable, Vim mode included.

## 12. How we prove "best" (evals)

- **Perf bench** (CI-gated): startup, latency, scroll fps, RAM, cat-100k — published vs Ghostty/Warp/Alacritty.
- **Agent suite** (~40 tasks): upgrade dep, fix failing test, write migration + verify, triage k8s log, AWS triage, dotfile setup. Metrics: success rate, steps, approvals-needed, tokens, undo-rate. Nightly vs Warp Agent + raw Claude Code in ZET.
- **Trust suite:** red-team prompts (prompt-injection via log output, `curl|sh`, prod-kubectl) — must require approval + sandbox, 100% block on T3 bypass.

## 13. Roadmap

- **M0 MVP (6 wks):** fast shell (pty+gpu+blocks), inline NL→cmd, single-lane agent with T0–T2 approvals + undo, local autocomplete, 1 cloud provider, MCP client.
- **M1 (6 wks):** planner+verifier loop, diff review, recipes, daemon + delegated runs, SSH remote agent, usage/audit.
- **1.0:** multi-agent lanes + panel, third-party CLI panes, team recipes + SSO/RBAC, perf parity with Ghostty on mac + linux, Windows beta.
- **2.0:** cloud factory (scheduled triggers, Slack/Linear/GitHub), benchmarks-as-product, local-8B fully offline agent.

## 14. Open decisions / risks

1. wgpu vs native Metal/OpenGL for absolute lowest latency — prototype both, keep `zet-gpu` backend trait.
2. cosmic-text vs harfbuzz shaping quality/perf — bench with Nerd + CJK.
3. Local model size for <50ms suggest on low-end laptops — 0.5B vs 1.7B quantized.
4. Block virtualization format — SQLite + Arrow vs pure memory-mapped ring.
5. Windows: ConPTY parity cost — defer until 1.0 if it risks mac/linux perf.

## Appendix A — CLI sketch

```bash
zet                              # open window
zet "fix failing e2e in ./api"    # open with agent goal
zet run --bg recipe:triage-ci --arg service=api
zet status; zet logs <run>; zet attach <run>
zet replay <session>               # replay timeline
zet undo [--step N]                # revert last agent checkpoint
zet audit <session>                # every cmd/diff/approval
zet recipe new upgrade-tailwind
```

## Appendix B — Repo layout (target)

```
zet/
  Cargo.toml          # workspace
  crates/{zet,zet-term,zet-pty,zet-gpu,zet-blocks,zet-context,zet-agent,zet-policy,zet-models,zet-mcp,zet-daemon}/
  docs/ZET-DESIGN.md  # this file
  zet-architecture.html
  recipes/
  benches/
  e2e/agent-suite/
```
