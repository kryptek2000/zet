//! Experimental `libghostty-vt` terminal engine.
//!
//! This module is compiled only with `--features ghostty-vt`. It is not the
//! production terminal engine and is activated only by
//! `ZET_TERMINAL_ENGINE=ghostty`.

use std::ffi::c_void;

use zet_gpu::quad::Rgba;
pub type GhosttyCell = zet_gpu::renderer::GhosttyCell;
pub type GhosttyFrame = zet_gpu::renderer::GhosttyFrame;

#[repr(C)]
struct GhosttyStyleColor {
    tag: u8,
    value: GhosttyStyleColorValue,
}

#[repr(C)]
union GhosttyStyleColorValue {
    palette: u8,
    rgb: GhosttyColorRgb,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyColorRgb {
    r: u8,
    g: u8,
    b: u8,
}

#[repr(C)]
struct GhosttyStyle {
    fg_color: GhosttyStyleColor,
    bg_color: GhosttyStyleColor,
    underline_color: GhosttyStyleColor,
    flags: u16,
    reserved: u16,
}

type GhosttyTerminalHandle = *mut c_void;
type GhosttyRenderState = *mut c_void;
type GhosttyRenderStateRowIterator = *mut c_void;
type GhosttyRenderStateRowCells = *mut c_void;
type GhosttyResult = i32;

#[link(name = "ghostty-vt")]
extern "C" {
    fn ghostty_terminal_new(
        allocator: *const c_void,
        terminal: *mut GhosttyTerminalHandle,
        cols: u16,
        rows: u16,
    ) -> GhosttyResult;
    fn ghostty_terminal_free(terminal: GhosttyTerminalHandle);
    fn ghostty_terminal_vt_write(terminal: GhosttyTerminalHandle, data: *const u8, len: usize);
    fn ghostty_terminal_resize(
        terminal: GhosttyTerminalHandle,
        cols: u16,
        rows: u16,
        cw: u32,
        ch: u32,
    ) -> GhosttyResult;
    fn ghostty_render_state_new(
        allocator: *const c_void,
        state: *mut GhosttyRenderState,
    ) -> GhosttyResult;
    fn ghostty_render_state_free(state: GhosttyRenderState);
    fn ghostty_render_state_update(
        state: GhosttyRenderState,
        terminal: GhosttyTerminalHandle,
    ) -> GhosttyResult;
    fn ghostty_render_state_get(
        state: GhosttyRenderState,
        key: u32,
        out: *mut c_void,
    ) -> GhosttyResult;
    fn ghostty_render_state_row_iterator_new(
        allocator: *const c_void,
        out: *mut GhosttyRenderStateRowIterator,
    ) -> GhosttyResult;
    fn ghostty_render_state_row_iterator_free(it: GhosttyRenderStateRowIterator);
    fn ghostty_render_state_row_iterator_next(it: GhosttyRenderStateRowIterator) -> bool;
    fn ghostty_render_state_row_get(
        it: GhosttyRenderStateRowIterator,
        key: u32,
        out: *mut c_void,
    ) -> GhosttyResult;
    fn ghostty_render_state_row_cells_new(
        allocator: *const c_void,
        out: *mut GhosttyRenderStateRowCells,
    ) -> GhosttyResult;
    fn ghostty_render_state_row_cells_free(cells: GhosttyRenderStateRowCells);
    fn ghostty_render_state_row_cells_next(cells: GhosttyRenderStateRowCells) -> bool;
    fn ghostty_render_state_row_cells_get(
        cells: GhosttyRenderStateRowCells,
        key: u32,
        out: *mut c_void,
    ) -> GhosttyResult;
    fn ghostty_cell_get(cell: u64, key: u32, out: *mut c_void) -> GhosttyResult;
}

pub struct GhosttyTerminal {
    term: GhosttyTerminalHandle,
    render_state: GhosttyRenderState,
}

impl GhosttyTerminal {
    pub fn new(cols: u16, rows: u16) -> Result<Self, String> {
        unsafe {
            let mut term: GhosttyTerminalHandle = std::ptr::null_mut();
            let rc = ghostty_terminal_new(std::ptr::null(), &mut term, cols, rows);
            if rc != 0 {
                return Err(format!("ghostty_terminal_new failed: {rc}"));
            }
            let mut render_state: GhosttyRenderState = std::ptr::null_mut();
            let rc = ghostty_render_state_new(std::ptr::null(), &mut render_state);
            if rc != 0 {
                ghostty_terminal_free(term);
                return Err(format!("ghostty_render_state_new failed: {rc}"));
            }
            Ok(Self { term, render_state })
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        unsafe { ghostty_terminal_vt_write(self.term, bytes.as_ptr(), bytes.len()) }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        unsafe {
            let _ = ghostty_terminal_resize(self.term, cols, rows, 8, 16);
        }
    }

    pub fn frame(&mut self) -> GhosttyFrame {
        unsafe {
            let _ = ghostty_render_state_update(self.render_state, self.term);

            let mut cols: u16 = 0;
            let mut rows: u16 = 0;
            let _ =
                ghostty_render_state_get(self.render_state, 1, &mut cols as *mut _ as *mut c_void);
            let _ =
                ghostty_render_state_get(self.render_state, 2, &mut rows as *mut _ as *mut c_void);

            #[repr(C)]
            #[derive(Clone, Copy)]
            struct Cursor {
                size: usize,
                viewport_has_value: bool,
                viewport_x: u16,
                viewport_y: u16,
                wide_tail: bool,
                visible: bool,
                blinking: bool,
                password_input: bool,
                visual_style: i32,
            }
            let mut cursor = Cursor {
                size: std::mem::size_of::<Cursor>(),
                viewport_has_value: false,
                viewport_x: 0,
                viewport_y: 0,
                wide_tail: false,
                visible: false,
                blinking: false,
                password_input: false,
                visual_style: 0,
            };
            let _ = ghostty_render_state_get(
                self.render_state,
                18,
                &mut cursor as *mut _ as *mut c_void,
            );

            let mut cells_handle: GhosttyRenderStateRowCells = std::ptr::null_mut();
            let _ = ghostty_render_state_row_cells_new(std::ptr::null(), &mut cells_handle);

            let mut frame_cells = vec![
                zet_gpu::renderer::GhosttyCell {
                    ch: ' ',
                    fg: Rgba::rgb(0.85, 0.85, 0.85),
                    bg: None,
                    bold: false,
                    inverse: false,
                };
                (cols as usize) * (rows as usize)
            ];

            let mut it: GhosttyRenderStateRowIterator = std::ptr::null_mut();
            let _ = ghostty_render_state_row_iterator_new(std::ptr::null(), &mut it);
            let _ =
                ghostty_render_state_get(self.render_state, 4, &mut it as *mut _ as *mut c_void);

            let mut row_idx = 0usize;
            while row_idx < rows as usize && ghostty_render_state_row_iterator_next(it) {
                let _ =
                    ghostty_render_state_row_get(it, 3, &mut cells_handle as *mut _ as *mut c_void);
                let mut col_idx = 0usize;
                while col_idx < cols as usize && ghostty_render_state_row_cells_next(cells_handle) {
                    let mut raw: u64 = 0;
                    let _ = ghostty_render_state_row_cells_get(
                        cells_handle,
                        1,
                        &mut raw as *mut _ as *mut c_void,
                    );
                    let mut cp: u32 = 0;
                    if raw != 0 {
                        let _ = ghostty_cell_get(raw, 1, &mut cp as *mut _ as *mut c_void);
                    }
                    let mut style = GhosttyStyle {
                        fg_color: GhosttyStyleColor {
                            tag: 0,
                            value: GhosttyStyleColorValue { palette: 0 },
                        },
                        bg_color: GhosttyStyleColor {
                            tag: 0,
                            value: GhosttyStyleColorValue { palette: 0 },
                        },
                        underline_color: GhosttyStyleColor {
                            tag: 0,
                            value: GhosttyStyleColorValue { palette: 0 },
                        },
                        flags: 0,
                        reserved: 0,
                    };
                    let _ = ghostty_render_state_row_cells_get(
                        cells_handle,
                        2,
                        &mut style as *mut _ as *mut c_void,
                    );

                    let idx = row_idx * (cols as usize) + col_idx;
                    if idx < frame_cells.len() {
                        frame_cells[idx].ch = char::from_u32(cp).unwrap_or(' ');
                        frame_cells[idx].fg =
                            resolve_color(style.fg_color, Rgba::rgb(0.85, 0.85, 0.85));
                        if style.bg_color.tag != 0 {
                            frame_cells[idx].bg =
                                Some(resolve_color(style.bg_color, Rgba::rgb(0.0, 0.0, 0.0)));
                        }
                        frame_cells[idx].bold = style.flags & 1 != 0;
                        frame_cells[idx].inverse = style.flags & (1 << 4) != 0;
                    }
                    col_idx += 1;
                }
                row_idx += 1;
            }

            ghostty_render_state_row_cells_free(cells_handle);
            ghostty_render_state_row_iterator_free(it);

            GhosttyFrame {
                cols: cols as usize,
                rows: rows as usize,
                cells: frame_cells,
                cursor_x: if cursor.viewport_has_value {
                    cursor.viewport_x as usize
                } else {
                    0
                },
                cursor_y: if cursor.viewport_has_value {
                    cursor.viewport_y as usize
                } else {
                    0
                },
                cursor_visible: cursor.visible,
            }
        }
    }
}

impl Drop for GhosttyTerminal {
    fn drop(&mut self) {
        unsafe {
            ghostty_render_state_free(self.render_state);
            ghostty_terminal_free(self.term);
        }
    }
}

fn palette_color(idx: u8) -> Rgba {
    let table: [Rgba; 16] = [
        Rgba::rgb(0.0, 0.0, 0.0),
        Rgba::rgb(0.8, 0.0, 0.0),
        Rgba::rgb(0.0, 0.8, 0.0),
        Rgba::rgb(0.8, 0.8, 0.0),
        Rgba::rgb(0.0, 0.0, 0.8),
        Rgba::rgb(0.8, 0.0, 0.8),
        Rgba::rgb(0.0, 0.8, 0.8),
        Rgba::rgb(0.8, 0.8, 0.8),
        Rgba::rgb(0.3, 0.3, 0.3),
        Rgba::rgb(1.0, 0.3, 0.3),
        Rgba::rgb(0.3, 1.0, 0.3),
        Rgba::rgb(1.0, 1.0, 0.3),
        Rgba::rgb(0.3, 0.3, 1.0),
        Rgba::rgb(1.0, 0.3, 1.0),
        Rgba::rgb(0.3, 1.0, 1.0),
        Rgba::rgb(1.0, 1.0, 1.0),
    ];
    if (idx as usize) < table.len() {
        table[idx as usize]
    } else if idx >= 232 {
        let v = (idx as f32 - 232.0) / 23.0;
        Rgba::rgb(v, v, v)
    } else {
        let i = idx as usize - 16;
        let r = (i / 36) as f32 / 5.0;
        let g = ((i / 6) % 6) as f32 / 5.0;
        let b = (i % 6) as f32 / 5.0;
        Rgba::rgb(r, g, b)
    }
}

fn resolve_color(c: GhosttyStyleColor, fallback: Rgba) -> Rgba {
    match c.tag {
        0 => fallback,
        1 => palette_color(unsafe { c.value.palette }),
        2 => {
            let v = unsafe { c.value.rgb };
            Rgba::rgb(v.r as f32 / 255.0, v.g as f32 / 255.0, v.b as f32 / 255.0)
        }
        _ => fallback,
    }
}
