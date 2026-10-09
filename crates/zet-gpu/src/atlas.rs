// Glyph atlas allocator for monospace terminal cells.
// Fixed-size slots in a grid; free-list reuse. Pure logic — the wgpu
// texture upload happens in the renderer, keyed by these rects.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub struct Atlas {
    cols: u32,
    rows: u32,
    cell_w: u32,
    cell_h: u32,
    free: Vec<(u32, u32)>,
}

impl Atlas {
    pub fn new(cols: u32, rows: u32, cell_w: u32, cell_h: u32) -> Self {
        assert!(cell_w > 0 && cell_h > 0, "cell dimensions must be nonzero");
        let mut free = Vec::with_capacity((cols * rows) as usize);
        for y in 0..rows {
            for x in 0..cols {
                free.push((x, y));
            }
        }
        free.reverse();
        Atlas {
            cols,
            rows,
            cell_w,
            cell_h,
            free,
        }
    }

    pub fn alloc(&mut self) -> Option<Slot> {
        let (x, y) = self.free.pop()?;
        Some(Slot {
            x: x * self.cell_w,
            y: y * self.cell_h,
            w: self.cell_w,
            h: self.cell_h,
        })
    }

    pub fn free(&mut self, slot: Slot) {
        // Reject slots that don't align to the grid: without this, a
        // fabricated slot aliases a live cell and corrupts the free list.
        if !slot.x.is_multiple_of(self.cell_w) || !slot.y.is_multiple_of(self.cell_h) {
            return;
        }
        let gx = slot.x / self.cell_w;
        let gy = slot.y / self.cell_h;
        if gx < self.cols && gy < self.rows && !self.free.contains(&(gx, gy)) {
            self.free.push((gx, gy));
        }
    }

    pub fn free_slots(&self) -> usize {
        self.free.len()
    }
}
