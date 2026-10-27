# P2-B1 — Performance Observations

**Date:** 2026-10-25
**Environment:** Hyprland 0.56.2 nested Wayland session, 1920 x 1080,
scale 1, Vulkan/Intel, `cargo build -p zet` **debug** profile.
**Before binary:** built from `origin/master` at
`d9bca1bc10a3e3c232a0b13b80b12701ddb5265b` (`zet-master`).
**After binary:** built from `work/p2b1-native-ui-slice` (`zet`).

These are debug-build measurements with deliberately small samples. They are
useful for spotting regressions, but they are **not** release-gate evidence.
The release gates remain `AGENTS.md` §perf: startup < 300 ms, input < 3 ms,
idle RSS < 100 MB, `cat 100k` < 0.9 s.

## Measurements

| Metric | Before | After | Observation |
|---|---|---|---|
| First frame (`ZET_PERF=1`) | 245, 248, 294, 328 ms across runs | 259, 260, 325, 332 ms across runs | Ranges overlap; debug-build startup is noisy. No consistent regression observed. |
| Single-key input latency | 4.853 ms | 3.058 ms | One sample each; debug build. No regression observed; below the 3 ms gate is **not** claimed. |
| Idle RSS, 3 tabs, ~11 s | 83,260 KB | 92,784 KB | +9.5 MB in the unoptimized debug binary. |
| Resident `.text` (`pmap`) | 16,984 KB | 25,760 KB | Accounts for essentially all of the RSS delta. |
| Anonymous heap (`pmap`) | 14,632 KB | 14,720 KB | +88 KB; no heap growth attributable to the rail. |

The after process was compared at the same three-tab state and the same
temporary HOME. `cat 100k` output throughput was not measured in this
environment; the terminal parser, PTY reader, scrollback, selection and
rendering path were not changed.

## Interpretation

- The new rail draws a fixed, small number of additional quads per frame (one
  rule, one footer fill, one or two fills for the active/selected card, and the
  text cells already covered by the row loop). It does not clone terminal
  content or add a timer/thread/dependency.
- The debug RSS delta is predominantly resident executable text, not heap.
  Release builds strip and lay out code differently; a release measurement is
  required before owner sign-off. The debug result is still below the 100 MB
  idle gate.
- Startup and input samples are too noisy to claim a win or a regression from
  this slice. The release benchmark on the exact PR head should be the
  authority.

## Reproduction

```bash
# nested Hyprland is a test harness, not part of ZET
WAYLAND_DISPLAY=wayland-2 ZET_PERF=1 \
  HOME=/tmp/opencode/p2b1/home ./target/debug/zet

# RSS and resident text pages
ps -p "$PID" -o pid,rss,etime
pmap -x "$PID" | tail -1
```

The exact commands and pids used for this run are recorded in the PR
verification notes; no synthetic benchmark numbers are presented here.
