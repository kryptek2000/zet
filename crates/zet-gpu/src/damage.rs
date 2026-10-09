// Damage tracking: only re-upload re-present changed regions.
// Pure logic, no GPU needed — the renderer drains this per frame.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Rect { x, y, w, h }
    }

    fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let x2 = (self.x + self.w).max(other.x + other.w);
        let y2 = (self.y + self.h).max(other.y + other.h);
        Rect {
            x,
            y,
            w: x2 - x,
            h: y2 - y,
        }
    }
}

#[derive(Debug, Default)]
pub struct Damage {
    rects: Vec<Rect>,
}

impl Damage {
    pub fn new() -> Self {
        Damage { rects: Vec::new() }
    }

    pub fn add(&mut self, mut rect: Rect) {
        let mut i = 0;
        while i < self.rects.len() {
            if rect.overlaps(&self.rects[i]) {
                let merged = rect.union(&self.rects[i]);
                self.rects.remove(i);
                rect = merged;
            } else {
                i += 1;
            }
        }
        self.rects.push(rect);
    }

    pub fn take(&mut self) -> Vec<Rect> {
        std::mem::take(&mut self.rects)
    }
}
