# P4-A2 F4 Visual Evidence Manifest

Status: VERIFIED

The five screenshots in this directory are real AGENTS captures, not SESSION
substitutions. They were captured after the F4 repair and independently
inspected via OCR for "AGENT ACTIVITY".

Source commit audited: 82712fb7544ac1d3d24ba75e808afe661b6f8b3d
Evidence commit: 9ec91082c2b4407a1c11acb9d673ee8d4cd72121
Binary identity: target/release/zet built from the above commit.

Capture method: X11 backend (WINIT_UNIX_BACKEND=x11), grim full-screen capture,
cropped to the ZET window using compositor-reported geometry and 1.5x scale.
Inspector mode: AGENTS (native UI displays "AGENT ACTIVITY").
ZET_HOME: isolated temporary directory per capture.

Images:
- 01-clean-agents.png: AGENTS view, no false red evidence-loss warning.
- 02-populated-agents.png: populated AGENTS list with a recorded opencode candidate.
- 03-selected-observation.png: selected observation with readable source and session evidence.
- 04-expanded-evidence.png: Enter-expanded evidence details including UNKNOWN states.
- 05-narrow-agents.png: minimum supported wide inspector view (no critical overlap).

Limitations:
- Incomplete-evidence state is not included as a separate screenshot; the code
  path and tests cover it.
- Narrow state uses the minimum compositor-supported window width.

No Rust source code was modified for this evidence correction.
