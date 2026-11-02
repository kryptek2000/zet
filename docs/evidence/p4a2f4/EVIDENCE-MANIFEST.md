# P4-A2 F4 Visual Evidence Manifest

Status: UNVERIFIED / BLOCKED

The five screenshots previously committed under this directory were invalid:
they showed the SESSION view rather than AGENTS. They have been removed.

Safe graphical capture could not be completed in this environment without
risking the owner's active desktop session. A Hyprland compositor IPC call
during capture verification hung the compositor; no further synthetic input
was sent after that point.

No Rust source code was modified for this evidence correction.

Source commit audited: 82712fb7544ac1d3d24ba75e808afe661b6f8b3d
Binary identity: target/release/zet built from the above commit (rebuilt after evidence-only changes).

Unverified states:
- 01 clean AGENTS without false red warning
- 02 populated AGENTS list
- 03 selected observation detail
- 04 expanded evidence
- 05 narrow AGENTS

These remain to be captured in a safe, isolated graphical session or by the
owner manually. No SESSION screenshot is substituted for AGENTS evidence.
