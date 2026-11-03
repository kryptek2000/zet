//! Pure mouse-wheel routing and protocol encoding used by the native window.
//! Separating this decision from PTY writes lets tests exercise actual wire bytes.

use zet_gpu::selection::CellPos;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WheelAction {
    Report(Vec<Vec<u8>>),
    AltArrows { up: bool, count: usize },
    Scroll(isize),
    Ignore,
}

/// Input coordinates must be terminal-relative (viewport-local), not the
/// absolute row addresses used for scrollback selection.
pub(super) fn wheel_action(
    mouse_mode: u16,
    mouse_sgr: bool,
    alt_active: bool,
    lines: isize,
    cell: Option<CellPos>,
) -> WheelAction {
    if lines == 0 {
        return WheelAction::Ignore;
    }
    let count = lines.saturating_abs().min(10) as usize;
    if mouse_mode != 0 {
        let Some(cell) = cell else {
            return WheelAction::Ignore;
        };
        let col = cell.col.saturating_add(1);
        let row = cell.row.saturating_add(1);
        let button = if lines > 0 { 64u8 } else { 65u8 };
        let event = if mouse_sgr {
            format!("\x1b[<{button};{col};{row}M").into_bytes()
        } else {
            // X10 uses the 1-based coordinate plus a 32-byte bias.
            let legacy = |position: usize| position.saturating_add(32).min(255) as u8;
            vec![0x1b, b'[', b'M', button + 32, legacy(col), legacy(row)]
        };
        return WheelAction::Report(vec![event; count]);
    }
    if alt_active {
        if cell.is_none() {
            return WheelAction::Ignore;
        }
        return WheelAction::AltArrows {
            up: lines > 0,
            count,
        };
    }
    WheelAction::Scroll(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(col: usize, row: usize) -> Option<CellPos> {
        Some(CellPos { col, row })
    }

    #[test]
    fn sgr_wheel_up_and_down_encode_exact_xterm_bytes() {
        assert_eq!(
            wheel_action(1000, true, true, 1, at(0, 0)),
            WheelAction::Report(vec![b"\x1b[<64;1;1M".to_vec()])
        );
        assert_eq!(
            wheel_action(1002, true, false, -1, at(7, 3)),
            WheelAction::Report(vec![b"\x1b[<65;8;4M".to_vec()])
        );
    }

    #[test]
    fn legacy_wheel_encoding_uses_one_based_coordinates_plus_32() {
        assert_eq!(
            wheel_action(1003, false, true, 1, at(0, 0)),
            WheelAction::Report(vec![vec![0x1b, b'[', b'M', 0x60, 33, 33]])
        );
        assert_eq!(
            wheel_action(1000, false, true, -1, at(7, 3)),
            WheelAction::Report(vec![vec![0x1b, b'[', b'M', 0x61, 40, 36]])
        );
    }

    #[test]
    fn legacy_coordinates_are_safely_capped() {
        assert_eq!(
            wheel_action(1000, false, false, 1, at(222, 400)),
            WheelAction::Report(vec![vec![0x1b, b'[', b'M', 0x60, 255, 255]])
        );
    }

    #[test]
    fn zero_distance_never_forwards_input() {
        for mode in [0, 1000, 1002, 1003] {
            assert_eq!(
                wheel_action(mode, true, true, 0, at(0, 0)),
                WheelAction::Ignore
            );
        }
    }

    #[test]
    fn off_viewport_pointer_never_forwards_to_pty() {
        for mode in [1000, 1002, 1003] {
            assert_eq!(
                wheel_action(mode, true, true, 3, None),
                WheelAction::Ignore
            );
        }
        assert_eq!(
            wheel_action(0, false, true, -3, None),
            WheelAction::Ignore
        );
    }

    #[test]
    fn alternate_screen_without_mouse_tracking_keeps_arrow_fallback() {
        assert_eq!(
            wheel_action(0, false, true, 3, at(10, 5)),
            WheelAction::AltArrows { up: true, count: 3 }
        );
        assert_eq!(
            wheel_action(0, false, true, -3, at(10, 5)),
            WheelAction::AltArrows { up: false, count: 3 }
        );
    }

    #[test]
    fn ordinary_shell_scrollback_does_not_emit_pty_events() {
        assert_eq!(
            wheel_action(0, false, false, 3, None),
            WheelAction::Scroll(3)
        );
        assert_eq!(
            wheel_action(0, false, false, -3, at(2, 2)),
            WheelAction::Scroll(-3)
        );
    }

    #[test]
    fn tracking_applies_on_primary_and_alternate_screens() {
        for alt in [false, true] {
            assert_eq!(
                wheel_action(1000, true, alt, 1, at(5, 6)),
                WheelAction::Report(vec![b"\x1b[<64;6;7M".to_vec()])
            );
        }
    }

    #[test]
    fn wheel_events_are_bounded_to_ten_per_input() {
        let expected = vec![b"\x1b[<65;2;3M".to_vec(); 10];
        assert_eq!(
            wheel_action(1000, true, true, -1000, at(1, 2)),
            WheelAction::Report(expected)
        );
        assert_eq!(
            wheel_action(0, false, true, isize::MIN, at(1, 2)),
            WheelAction::AltArrows { up: false, count: 10 }
        );
    }

    #[test]
    fn protocol_coordinates_ignore_scrollback_offsets() {
        let vp = zet_gpu::renderer::Viewport {
            x: 30,
            y: 4,
            cols: 80,
            rows: 20,
        };
        let x = 30.0 * 10.0 + 25.0;
        let y = 4.0 * 20.0 + 85.0;
        let local = zet_gpu::renderer::viewport_hit_test(x, y, vp, 10, 20, 0);
        let selection = zet_gpu::renderer::viewport_hit_test(x, y, vp, 10, 20, 50);
        assert_eq!(local, at(2, 4));
        assert_eq!(selection, at(2, 54));
        assert_eq!(
            wheel_action(1000, true, false, 1, local),
            WheelAction::Report(vec![b"\x1b[<64;3;5M".to_vec()])
        );
        assert!(zet_gpu::renderer::viewport_hit_test(299.0, y, vp, 10, 20, 0).is_none());
    }
}
