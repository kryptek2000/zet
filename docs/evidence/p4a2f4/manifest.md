# P4-A2 F4 AGENTS Visual Evidence Manifest

Source commit: 82712fb7544ac1d3d24ba75e808afe661b6f8b3d
Binary: target/release/zet built from 82712fb
Capture method: X11 backend (WINIT_UNIX_BACKEND=x11), grim full-screen capture, cropped to the ZET window using compositor-reported geometry and 1.5x scale.
Inspector mode: AGENTS (native UI displays "AGENT ACTIVITY").
ZET_HOME: isolated temporary directory per capture.

Images:
- 01-clean-agents.png: AGENTS view, no false red evidence-loss warning.
- 02-populated-agents.png: populated AGENTS list with a recorded opencode candidate.
- 03-selected-observation.png: selected observation with readable source and session evidence.
- 04-expanded-evidence.png: Enter-expanded evidence details including UNKNOWN states.
- 05-narrow-agents.png: minimum supported wide inspector view (resize granularity limited; no critical overlap).

Limitations:
- Incomplete-evidence state is not included as a separate screenshot in this set; the code path and tests are covered by unit tests.
- Narrow state uses the minimum compositor-supported window width.
