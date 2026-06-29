// ── Window ────────────────────────────────────────────────────────────────────

struct Window {
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    minimized: bool,
    last_refresh_ms: u64,
    app: Box<dyn App>,
    // ── Surface cache ─────────────────────────────────────────────────
    // Stores the last-rendered client-area pixels (row-major, ARGB32).
    // When valid, compose_damage can blit this instead of calling app.render(),
    // which eliminates redundant text/background rendering during drag/resize.
    cached_surface: Vec<u32>,
    surface_valid: bool,         // false = must call app.render() and re-capture
    surface_w: usize,            // client width at capture time
    surface_h: usize,            // client height at capture time
    surface_needs_capture: bool, // set after a full render; triggers read_rect capture
}

impl Window {
    fn client_rect(&self) -> Rect {
        let x = (self.x.max(0) as usize) + 1;
        let y = (self.y.max(0) as usize) + WIN_BAR_H + 1;
        let w = self.w.saturating_sub(2);
        let h = self.h.saturating_sub(WIN_BAR_H + 2);
        Rect { x, y, w, h }
    }

    fn bounds(&self) -> Rect {
        Rect {
            x: self.x.max(0) as usize,
            y: self.y.max(0) as usize,
            w: self.w + WIN_SHADOW_OFS,
            h: self.h + WIN_SHADOW_OFS,
        }
    }

    fn close_btn_rect(&self) -> Rect {
        let wx = self.x.max(0) as usize;
        let wy = self.y.max(0) as usize;
        let bw = 16usize;
        let bh = 16usize;
        let bx = wx + self.w.saturating_sub(bw + 6);
        let by = wy + (WIN_BAR_H.saturating_sub(bh)) / 2;
        Rect {
            x: bx,
            y: by,
            w: bw,
            h: bh,
        }
    }
}

// ── Resize zone ───────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq)]
enum ResizeZone {
    TL,
    T,
    TR,
    R,
    BR,
    B,
    BL,
    L,
}

impl ResizeZone {
    fn cursor_shape(self) -> CursorShape {
        match self {
            ResizeZone::TL | ResizeZone::BR => CursorShape::ResizeDiagA,
            ResizeZone::TR | ResizeZone::BL => CursorShape::ResizeDiagB,
            ResizeZone::L | ResizeZone::R => CursorShape::ResizeH,
            ResizeZone::T | ResizeZone::B => CursorShape::ResizeV,
        }
    }
}

fn hit_resize_zone(win: &Window, mx: i32, my: i32) -> Option<ResizeZone> {
    let wx = win.x;
    let wy = win.y;
    let ww = win.w as i32;
    let wh = win.h as i32;
    let z = RESIZE_ZONE as i32;
    if mx < wx || mx > wx + ww || my < wy || my > wy + wh {
        return None;
    }
    let lft = mx - wx < z;
    let rgt = wx + ww - mx < z;
    let top = my - wy < z;
    let bot = wy + wh - my < z;
    if !lft && !rgt && !top && !bot {
        return None;
    }
    if top && my - wy < WIN_BAR_H as i32 {
        return None;
    }
    match (lft, rgt, top, bot) {
        (true, false, true, false) => Some(ResizeZone::TL),
        (false, false, true, false) => Some(ResizeZone::T),
        (false, true, true, false) => Some(ResizeZone::TR),
        (false, true, false, false) => Some(ResizeZone::R),
        (false, true, false, true) => Some(ResizeZone::BR),
        (false, false, false, true) => Some(ResizeZone::B),
        (true, false, false, true) => Some(ResizeZone::BL),
        (true, false, false, false) => Some(ResizeZone::L),
        _ => None,
    }
}

