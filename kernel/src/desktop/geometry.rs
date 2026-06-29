// ── Damage tracking ───────────────────────────────────────────────────────────

#[derive(Copy, Clone)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Rect {
    const ZERO: Self = Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }

    fn union(&self, o: &Rect) -> Rect {
        if self.w == 0 || self.h == 0 {
            return *o;
        }
        if o.w == 0 || o.h == 0 {
            return *self;
        }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = (self.x + self.w).max(o.x + o.w);
        let y1 = (self.y + self.h).max(o.y + o.h);
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }

    fn clip(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        if x0 >= x1 || y0 >= y1 {
            Rect::ZERO
        } else {
            Rect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
}

struct DamageList {
    rects: [Rect; MAX_DAMAGE],
    count: usize,
    full: bool,
}

impl DamageList {
    fn new() -> Self {
        DamageList {
            rects: [Rect::ZERO; MAX_DAMAGE],
            count: 0,
            full: false,
        }
    }
    fn clear(&mut self) {
        self.count = 0;
        self.full = false;
    }
    fn mark_full(&mut self) {
        self.full = true;
    }
    fn is_empty(&self) -> bool {
        !self.full && self.count == 0
    }

    fn add(&mut self, r: Rect) {
        if r.w == 0 || r.h == 0 || self.full {
            return;
        }
        for i in 0..self.count {
            if self.rects[i].intersects(&r) {
                self.rects[i] = self.rects[i].union(&r);
                self.cascade(i);
                return;
            }
        }
        if self.count < MAX_DAMAGE {
            self.rects[self.count] = r;
            self.count += 1;
        } else {
            self.full = true;
        }
    }

    fn cascade(&mut self, idx: usize) {
        let mut i = 0;
        while i < self.count {
            if i != idx && self.rects[idx].intersects(&self.rects[i]) {
                self.rects[idx] = self.rects[idx].union(&self.rects[i]);
                self.count -= 1;
                if i < self.count {
                    self.rects[i] = self.rects[self.count];
                }
            } else {
                i += 1;
            }
        }
    }
}
