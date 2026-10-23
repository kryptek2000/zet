//! Pure, deterministic layout resolution for the Mission Control chrome.
//!
//! This module knows nothing about GPUs, PTYs, windows or I/O. It only
//! turns a window's cell dimensions plus two visibility toggles into a set
//! of non-overlapping [`Viewport`]s. Keeping it pure makes every geometry
//! rule below unit-testable without a display.
//!
//! The live application still renders a single full-width grid; wiring this
//! resolver into `App` is deliberately deferred to a later slice so that
//! Slice 1 changes no rendering or PTY behaviour.

/// Default width of the mission rail, in terminal columns.
pub const RAIL_COLS: u32 = 24;
/// Default width of the inspector, in terminal columns.
pub const INSPECTOR_COLS: u32 = 32;
/// The terminal never gets narrower than this, even if a panel has to be
/// refused.
pub const MIN_TERM_COLS: u32 = 40;
/// The terminal never gets shorter than this.
pub const MIN_TERM_ROWS: u32 = 10;

/// Which chrome regions are visible. Both flags are independent; the named
/// layouts are just presets over the same two flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub rail: bool,
    pub inspector: bool,
}

impl Layout {
    /// The named preset represented by these flags, if any.
    ///
    /// Inspector-only visibility is intentionally not a named preset.
    pub fn mode(&self) -> Option<LayoutMode> {
        LayoutMode::for_layout(*self)
    }

    /// Convenience resolver taking `self` as the requested layout.
    pub fn resolve(self, cols: u32, rows: u32, bar_rows: usize) -> FrameLayout {
        resolve(cols, rows, self, bar_rows)
    }
}

/// The three named presentations from the product charter. They are presets,
/// not separate runtimes: one live app, one PTY per tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    TerminalFocus,
    Workspace,
    MissionControl,
}

impl LayoutMode {
    /// The visibility preset for this mode.
    pub fn layout(self) -> Layout {
        match self {
            Self::TerminalFocus => Layout {
                rail: false,
                inspector: false,
            },
            Self::Workspace => Layout {
                rail: true,
                inspector: false,
            },
            Self::MissionControl => Layout {
                rail: true,
                inspector: true,
            },
        }
    }

    /// The named preset represented by `layout`, if it is one of the three.
    pub fn for_layout(layout: Layout) -> Option<Self> {
        match (layout.rail, layout.inspector) {
            (false, false) => Some(Self::TerminalFocus),
            (true, false) => Some(Self::Workspace),
            (true, true) => Some(Self::MissionControl),
            (false, true) => None,
        }
    }

    /// Cycle through the three presets in order.
    pub fn next(self) -> Self {
        match self {
            Self::TerminalFocus => Self::Workspace,
            Self::Workspace => Self::MissionControl,
            Self::MissionControl => Self::TerminalFocus,
        }
    }
}

impl From<LayoutMode> for Layout {
    fn from(mode: LayoutMode) -> Self {
        mode.layout()
    }
}

/// A cell rectangle inside the window grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub x: u32,
    pub y: u32,
    pub cols: u32,
    pub rows: u32,
}

/// Why a requested panel could not be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Degraded {
    /// The rail was requested but could not be shown.
    RailRefused,
    /// The inspector was requested but could not be shown.
    InspectorRefused,
    /// Both requested panels could not be shown.
    BothRefused,
}

/// Resolved geometry for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLayout {
    /// The layout that was actually requested.
    pub requested: Layout,
    /// The named preset represented by `requested`, if any. `None` means a
    /// custom/inspector-only visibility that is not one of the three charter
    /// presets. This is distinct from effective rendered visibility, which is
    /// captured by `rail`/`inspector`/`degraded`.
    pub mode: Option<LayoutMode>,
    pub rail: Option<Viewport>,
    pub term: Viewport,
    pub inspector: Option<Viewport>,
    pub degraded: Option<Degraded>,
}

impl FrameLayout {
    /// Resolve a frame layout from window dimensions and a requested layout.
    pub fn resolve<R>(cols: u32, rows: u32, requested: R, bar_rows: usize) -> Self
    where
        R: Into<Layout>,
    {
        resolve(cols, rows, requested, bar_rows)
    }
}

/// Pure deterministic layout resolver.
///
/// `cols`/`rows` are the window's cell dimensions. `bar_rows` is the number
/// of rows consumed by the tab bar (0 or 1 in practice) and is applied to
/// the vertical origin exactly once. `requested` is the desired visibility.
pub fn resolve<R>(cols: u32, rows: u32, requested: R, bar_rows: usize) -> FrameLayout
where
    R: Into<Layout>,
{
    let requested = requested.into();
    let mode = LayoutMode::for_layout(requested);
    resolve_with_mode(cols, rows, requested, mode, bar_rows)
}

/// Resolve with an explicitly supplied named preset.
///
/// The supplied preset is only retained when it actually represents
/// `requested`; otherwise the truthful preset derived from `requested` (or
/// `None`) is used. This prevents a contradictory mode label.
pub fn resolve_mode<R, M>(
    cols: u32,
    rows: u32,
    requested: R,
    mode: M,
    bar_rows: usize,
) -> FrameLayout
where
    R: Into<Layout>,
    M: Into<Option<LayoutMode>>,
{
    let requested = requested.into();
    let supplied = mode.into();
    let mode = supplied
        .filter(|candidate| candidate.layout() == requested)
        .or_else(|| LayoutMode::for_layout(requested));
    resolve_with_mode(cols, rows, requested, mode, bar_rows)
}

/// Resolve a named preset directly.
pub fn resolve_preset(cols: u32, rows: u32, mode: LayoutMode, bar_rows: usize) -> FrameLayout {
    resolve(cols, rows, mode.layout(), bar_rows)
}

/// Backwards-friendly alias for [`resolve`].
/// Terminal viewport dimensions for a window. This is the seam used by
/// `App::fit_all` so PTY and grid sizing can be tested without a window.
pub fn terminal_viewport(cols: u32, rows: u32, bar_rows: usize) -> (u32, u32) {
    let viewport = resolve(cols, rows, LayoutMode::TerminalFocus.layout(), bar_rows).term;
    (viewport.cols, viewport.rows)
}

pub fn resolve_layout<R>(cols: u32, rows: u32, requested: R, bar_rows: usize) -> FrameLayout
where
    R: Into<Layout>,
{
    resolve(cols, rows, requested, bar_rows)
}

fn resolve_with_mode(
    cols: u32,
    rows: u32,
    requested: Layout,
    mode: Option<LayoutMode>,
    bar_rows: usize,
) -> FrameLayout {
    let mut rail_cols = if requested.rail { RAIL_COLS } else { 0 };
    let mut inspector_cols = if requested.inspector {
        INSPECTOR_COLS
    } else {
        0
    };

    // Collapse the inspector before the rail when space is insufficient.
    if requested.inspector
        && cols
            .saturating_sub(rail_cols)
            .saturating_sub(INSPECTOR_COLS)
            < MIN_TERM_COLS
    {
        inspector_cols = 0;
    }

    // Collapse the rail if the terminal would still be too narrow.
    if requested.rail && cols.saturating_sub(rail_cols) < MIN_TERM_COLS {
        rail_cols = 0;
    }

    let term_cols = cols
        .saturating_sub(rail_cols)
        .saturating_sub(inspector_cols);
    let effective_bar_rows = bar_rows.min(rows as usize);
    let term_rows = rows - effective_bar_rows as u32;
    let origin_y = effective_bar_rows as u32;

    let rail = if requested.rail && rail_cols > 0 {
        Some(Viewport {
            x: 0,
            y: origin_y,
            cols: rail_cols,
            rows: term_rows,
        })
    } else {
        None
    };

    let term = Viewport {
        x: rail_cols,
        y: origin_y,
        cols: term_cols,
        rows: term_rows,
    };

    let inspector = if requested.inspector && inspector_cols > 0 {
        Some(Viewport {
            x: rail_cols + term_cols,
            y: origin_y,
            cols: inspector_cols,
            rows: term_rows,
        })
    } else {
        None
    };

    let rail_refused = requested.rail && rail.is_none();
    let inspector_refused = requested.inspector && inspector.is_none();
    let degraded = match (rail_refused, inspector_refused) {
        (false, false) => None,
        (true, false) => Some(Degraded::RailRefused),
        (false, true) => Some(Degraded::InspectorRefused),
        (true, true) => Some(Degraded::BothRefused),
    };

    FrameLayout {
        requested,
        mode,
        rail,
        term,
        inspector,
        degraded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_focus_is_the_full_width_grid() {
        let layout = LayoutMode::TerminalFocus.layout();
        let frame = resolve(120, 40, layout, 0);
        assert_eq!(frame.requested, layout);
        assert_eq!(frame.mode, Some(LayoutMode::TerminalFocus));
        assert_eq!(frame.rail, None);
        assert_eq!(frame.inspector, None);
        assert_eq!(frame.term.x, 0);
        assert_eq!(frame.term.cols, 120);
        assert_eq!(frame.term.rows, 40);
        assert_eq!(frame.term.y, 0);
        assert_eq!(frame.degraded, None);
    }

    #[test]
    fn rail_and_inspector_split_the_columns() {
        let layout = LayoutMode::MissionControl.layout();
        let frame = resolve(120, 40, layout, 1);
        assert_eq!(frame.requested, layout);
        assert_eq!(frame.mode, Some(LayoutMode::MissionControl));
        assert_eq!(frame.rail.unwrap().cols, RAIL_COLS);
        assert_eq!(frame.inspector.unwrap().cols, INSPECTOR_COLS);
        assert_eq!(frame.term.cols, 120 - RAIL_COLS - INSPECTOR_COLS);
        assert_eq!(frame.term.x, RAIL_COLS);
        assert_eq!(frame.inspector.unwrap().x, RAIL_COLS + frame.term.cols);
        assert_eq!(frame.term.y, 1);
        assert_eq!(frame.term.rows, 39);
        assert_eq!(frame.rail.unwrap().y, 1);
        assert_eq!(frame.inspector.unwrap().y, 1);
        assert_eq!(frame.degraded, None);
    }

    #[test]
    fn inspector_is_refused_before_the_rail() {
        let layout = LayoutMode::MissionControl.layout();
        let frame = resolve(90, 40, layout, 0);
        assert!(frame.rail.is_some());
        assert!(frame.inspector.is_none());
        assert_eq!(frame.degraded, Some(Degraded::InspectorRefused));
        assert_eq!(frame.term.cols, 90 - RAIL_COLS);
    }

    #[test]
    fn both_are_refused_below_the_minimum() {
        let layout = LayoutMode::MissionControl.layout();
        let frame = resolve(50, 40, layout, 0);
        assert!(frame.rail.is_none());
        assert!(frame.inspector.is_none());
        assert_eq!(frame.degraded, Some(Degraded::BothRefused));
        assert_eq!(frame.term.cols, 50);
    }

    #[test]
    fn rail_alone_is_refused_when_too_narrow() {
        let layout = LayoutMode::Workspace.layout();
        let frame = resolve(50, 40, layout, 0);
        assert!(frame.rail.is_none());
        assert_eq!(frame.degraded, Some(Degraded::RailRefused));
        assert_eq!(frame.term.cols, 50);
    }

    #[test]
    fn terminal_never_drops_below_the_minimum_when_possible() {
        for cols in 20u32..=500 {
            let layout = LayoutMode::MissionControl.layout();
            let frame = resolve(cols, 40, layout, 0);
            if cols >= MIN_TERM_COLS + RAIL_COLS + INSPECTOR_COLS {
                assert!(frame.term.cols >= MIN_TERM_COLS);
                assert!(frame.rail.is_some());
                assert!(frame.inspector.is_some());
            }
            let used = frame.term.cols
                + frame.rail.map_or(0, |v| v.cols)
                + frame.inspector.map_or(0, |v| v.cols);
            assert_eq!(used, cols);
        }
    }

    #[test]
    fn the_tab_bar_row_is_counted_exactly_once() {
        let layout = LayoutMode::TerminalFocus.layout();
        for bar_rows in [0usize, 1] {
            let frame = resolve(80, 24, layout, bar_rows);
            assert_eq!(frame.term.y, bar_rows as u32);
            assert_eq!(frame.term.rows, 24 - bar_rows as u32);
            assert_eq!(frame.term.y, bar_rows as u32);
        }
    }

    #[test]
    fn mode_presets_round_trip() {
        assert!(!LayoutMode::TerminalFocus.layout().rail);
        assert!(!LayoutMode::TerminalFocus.layout().inspector);
        assert!(LayoutMode::Workspace.layout().rail);
        assert!(!LayoutMode::Workspace.layout().inspector);
        assert!(LayoutMode::MissionControl.layout().rail);
        assert!(LayoutMode::MissionControl.layout().inspector);

        let mut mode = LayoutMode::TerminalFocus;
        mode = mode.next();
        assert_eq!(mode, LayoutMode::Workspace);
        mode = mode.next();
        assert_eq!(mode, LayoutMode::MissionControl);
        mode = mode.next();
        assert_eq!(mode, LayoutMode::TerminalFocus);
    }

    #[test]
    fn narrow_windows_do_not_underflow() {
        for cols in 0u32..=39 {
            for rows in 0u32..=5 {
                let frame = resolve(cols, rows, LayoutMode::MissionControl.layout(), 1);
                let used = frame.term.cols
                    + frame.rail.map_or(0, |v| v.cols)
                    + frame.inspector.map_or(0, |v| v.cols);
                assert!(used <= cols);
                assert!(frame.term.rows <= rows);
            }
        }
    }

    #[test]
    fn no_regions_overlap() {
        for cols in [0u32, 1, 39, 40, 63, 64, 71, 72, 95, 96, 120, 200] {
            for bar_rows in [0usize, 1] {
                let frame = resolve(cols, 40, LayoutMode::MissionControl.layout(), bar_rows);
                let used = frame.term.cols
                    + frame.rail.map_or(0, |v| v.cols)
                    + frame.inspector.map_or(0, |v| v.cols);
                assert_eq!(used, cols);
                assert_eq!(frame.term.x, frame.rail.map_or(0, |v| v.cols));
                assert_eq!(
                    frame.term.x + frame.term.cols + frame.inspector.map_or(0, |v| v.cols),
                    cols
                );
                if let (Some(rail), Some(insp)) = (frame.rail, frame.inspector) {
                    assert!(rail.x + rail.cols <= frame.term.x);
                    assert!(frame.term.x + frame.term.cols <= insp.x);
                    assert_eq!(insp.x + insp.cols, cols);
                }
            }
        }
    }

    #[test]
    fn frame_layout_resolve_methods_agree() {
        let layout = LayoutMode::Workspace.layout();
        let a = resolve(100, 30, layout, 1);
        let b = FrameLayout::resolve(100, 30, layout, 1);
        let c = layout.resolve(100, 30, 1);
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(resolve_layout(100, 30, layout, 1), a);
        assert_eq!(resolve_preset(100, 30, LayoutMode::Workspace, 1), a);
        assert_eq!(
            resolve_mode(100, 30, layout, Some(LayoutMode::Workspace), 1),
            a
        );
    }

    #[test]
    fn inspector_refused_keeps_rail() {
        let layout = LayoutMode::MissionControl.layout();
        let frame = resolve(95, 40, layout, 0);
        assert!(frame.rail.is_some());
        assert!(frame.inspector.is_none());
        assert_eq!(frame.degraded, Some(Degraded::InspectorRefused));
    }

    #[test]
    fn all_three_modes_resolve() {
        for mode in [
            LayoutMode::TerminalFocus,
            LayoutMode::Workspace,
            LayoutMode::MissionControl,
        ] {
            let frame = resolve(160, 50, mode.layout(), 1);
            assert_eq!(frame.mode, Some(mode));
            assert!(frame.term.cols > 0);
            assert!(frame.term.rows > 0);
        }
    }

    #[test]
    fn min_term_rows_constant_is_ten() {
        assert_eq!(MIN_TERM_ROWS, 10);
    }

    #[test]
    fn inspector_only_has_no_named_preset() {
        let layout = Layout {
            rail: false,
            inspector: true,
        };
        let frame = resolve(120, 40, layout, 0);
        assert_eq!(frame.requested, layout);
        assert_eq!(frame.mode, None);
        assert!(frame.inspector.is_some());
    }

    #[test]
    fn contradictory_supplied_mode_is_not_retained() {
        let layout = Layout {
            rail: false,
            inspector: true,
        };
        let frame = resolve_mode(120, 40, layout, Some(LayoutMode::MissionControl), 0);
        assert_eq!(frame.mode, None);
    }

    #[test]
    fn terminal_focus_at_30_cols_has_no_degradation() {
        let frame = resolve(30, 24, LayoutMode::TerminalFocus.layout(), 0);
        assert_eq!(frame.degraded, None);
        assert_eq!(frame.term.cols, 30);
    }

    #[test]
    fn rail_only_boundary_cases() {
        let layout = Layout {
            rail: true,
            inspector: false,
        };
        let refused = resolve(63, 24, layout, 0);
        assert!(refused.rail.is_none());
        assert_eq!(refused.degraded, Some(Degraded::RailRefused));
        assert_eq!(refused.term.cols, 63);

        let shown = resolve(64, 24, layout, 0);
        assert!(shown.rail.is_some());
        assert_eq!(shown.degraded, None);
        assert_eq!(shown.term.cols, 64 - RAIL_COLS);
    }

    #[test]
    fn inspector_only_boundary_cases() {
        let layout = Layout {
            rail: false,
            inspector: true,
        };
        for cols in [30u32, 71] {
            let frame = resolve(cols, 24, layout, 0);
            assert!(frame.inspector.is_none());
            assert_eq!(frame.degraded, Some(Degraded::InspectorRefused));
            assert_eq!(frame.term.cols, cols);
        }
        let shown = resolve(72, 24, layout, 0);
        assert!(shown.inspector.is_some());
        assert_eq!(shown.degraded, None);
        assert_eq!(shown.term.cols, 72 - INSPECTOR_COLS);
    }

    #[test]
    fn both_requested_boundary_cases() {
        let layout = LayoutMode::MissionControl.layout();
        let cases = [
            (39u32, Degraded::BothRefused, true, true, 39u32),
            (40, Degraded::BothRefused, true, true, 40),
            (63, Degraded::BothRefused, true, true, 63),
            (64, Degraded::InspectorRefused, false, true, 40),
            (95, Degraded::InspectorRefused, false, true, 71),
            (96, Degraded::InspectorRefused, false, false, 40),
        ];
        // 96 is both visible, handled separately below.
        for (cols, degraded, rail_refused, inspector_refused, term_cols) in cases {
            if cols == 96 {
                continue;
            }
            let frame = resolve(cols, 24, layout, 0);
            assert_eq!(frame.degraded, Some(degraded));
            assert_eq!(frame.rail.is_none(), rail_refused);
            assert_eq!(frame.inspector.is_none(), inspector_refused);
            assert_eq!(frame.term.cols, term_cols);
        }

        let shown = resolve(96, 24, layout, 0);
        assert!(shown.rail.is_some());
        assert!(shown.inspector.is_some());
        assert_eq!(shown.degraded, None);
        assert_eq!(shown.term.cols, 40);
    }

    #[test]
    fn all_visibility_combinations_across_widths() {
        for rail in [false, true] {
            for inspector in [false, true] {
                let layout = Layout { rail, inspector };
                for cols in [0u32, 20, 39, 40, 63, 64, 71, 72, 95, 96, 120, 200, 500] {
                    let frame = resolve(cols, 30, layout, 1);
                    let used = frame.term.cols
                        + frame.rail.map_or(0, |v| v.cols)
                        + frame.inspector.map_or(0, |v| v.cols);
                    assert_eq!(used, cols);
                    assert!(frame.term.rows <= 30);
                    assert_eq!(frame.requested, layout);
                    // Truthfulness: degradation only names requested panels.
                    let rail_refused = rail && frame.rail.is_none();
                    let inspector_refused = inspector && frame.inspector.is_none();
                    let expected = match (rail_refused, inspector_refused) {
                        (false, false) => None,
                        (true, false) => Some(Degraded::RailRefused),
                        (false, true) => Some(Degraded::InspectorRefused),
                        (true, true) => Some(Degraded::BothRefused),
                    };
                    assert_eq!(frame.degraded, expected);
                }
            }
        }
    }

    #[test]
    fn bar_rows_greater_than_rows_does_not_underflow() {
        let layout = LayoutMode::TerminalFocus.layout();
        let frame = resolve(80, 5, layout, 10);
        assert_eq!(frame.term.rows, 0);
        assert_eq!(frame.term.y, 5); // clamped to available height
        assert_eq!(frame.term.cols, 80);
    }

    #[test]
    fn resolve_mode_rejects_contradictory_input() {
        let layout = Layout {
            rail: false,
            inspector: true,
        };
        let frame = resolve_mode(120, 40, layout, Some(LayoutMode::MissionControl), 0);
        assert_eq!(frame.mode, None);
        assert!(frame.inspector.is_some());
    }

    #[test]
    fn terminal_viewport_matches_frame_layout() {
        for cols in [0u32, 1, 20, 80, 104, 500] {
            for rows in [0u32, 1, 5, 24, 40] {
                for bar_rows in [0usize, 1, 5] {
                    let (vp_cols, vp_rows) = terminal_viewport(cols, rows, bar_rows);
                    let frame = resolve(cols, rows, LayoutMode::TerminalFocus.layout(), bar_rows);
                    assert_eq!((vp_cols, vp_rows), (frame.term.cols, frame.term.rows));
                }
            }
        }
    }

    #[test]
    fn terminal_viewport_narrower_than_window() {
        let (cols, rows) = terminal_viewport(104, 24, 1);
        assert_eq!((cols, rows), (104, 23));
        let (cols, rows) = terminal_viewport(104, 24, 0);
        assert_eq!((cols, rows), (104, 24));
    }

    #[test]
    fn viewport_origin_and_height_are_clamped_to_window() {
        for rows in [0u32, 1, 5] {
            for bar_rows in [0usize, 1, 5, 10, usize::MAX] {
                for rail in [false, true] {
                    for inspector in [false, true] {
                        let layout = Layout { rail, inspector };
                        let frame = resolve(80, rows, layout, bar_rows);
                        let effective = bar_rows.min(rows as usize) as u32;
                        assert_eq!(frame.term.y, effective);
                        assert_eq!(frame.term.rows, rows - effective);
                        assert!(frame.term.y <= rows);
                        assert!(frame.term.rows <= rows);
                        assert_eq!(frame.term.y + frame.term.rows, rows);
                        for viewport in [frame.rail, Some(frame.term), frame.inspector]
                            .into_iter()
                            .flatten()
                        {
                            assert!(viewport.y <= rows);
                            assert!(viewport.rows <= rows);
                            assert!(viewport.y + viewport.rows <= rows);
                        }
                        let used = frame.term.cols
                            + frame.rail.map_or(0, |v| v.cols)
                            + frame.inspector.map_or(0, |v| v.cols);
                        assert_eq!(used, 80);
                    }
                }
            }
        }
    }

    #[test]
    fn bar_rows_greater_than_rows_clamps_origin_and_height() {
        let layout = LayoutMode::TerminalFocus.layout();
        let frame = resolve(80, 5, layout, 10);
        assert_eq!(frame.term.y, 5);
        assert_eq!(frame.term.rows, 0);
        assert_eq!(frame.term.y + frame.term.rows, 5);
    }

    #[test]
    fn zero_height_window_is_safe() {
        let layout = LayoutMode::MissionControl.layout();
        let frame = resolve(80, 0, layout, 10);
        assert_eq!(frame.term.y, 0);
        assert_eq!(frame.term.rows, 0);
        assert_eq!(frame.term.y + frame.term.rows, 0);
    }

    #[test]
    fn usize_max_bar_rows_clamps_safely() {
        let layout = LayoutMode::Workspace.layout();
        let frame = resolve(80, 7, layout, usize::MAX);
        assert_eq!(frame.term.y, 7);
        assert_eq!(frame.term.rows, 0);
    }
}
