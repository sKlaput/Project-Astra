// ── Desktop context menu ─────────────────────────────────────────────────────

const DCTX_W: usize = 130;
const DCTX_ITEM_H: usize = 22;
const DCTX_ITEMS: usize = 2;
const DCTX_H: usize = DCTX_ITEMS * DCTX_ITEM_H + 4; // 2 px top/bottom padding
const DCTX_BG: u32 = 0x0A0F18;
const DCTX_BORD: u32 = 0x1A2F48;
const DCTX_HOV: u32 = 0x162840;
const DCTX_TEXT: u32 = 0xD8EEFF;

#[derive(Copy, Clone)]
struct DesktopCtxMenu {
    visible: bool,
    x: i32,
    y: i32,
    hover: Option<usize>,
}

impl DesktopCtxMenu {
    const fn hidden() -> Self {
        DesktopCtxMenu {
            visible: false,
            x: 0,
            y: 0,
            hover: None,
        }
    }

    fn rect(&self, sw: usize, sh: usize) -> Rect {
        let x = (self.x as usize).min(sw.saturating_sub(DCTX_W));
        let y = (self.y as usize).min(sh.saturating_sub(DCTX_H));
        Rect {
            x,
            y,
            w: DCTX_W,
            h: DCTX_H,
        }
    }

    fn item_rect(&self, i: usize, sw: usize, sh: usize) -> Rect {
        let r = self.rect(sw, sh);
        Rect {
            x: r.x + 1,
            y: r.y + 2 + i * DCTX_ITEM_H,
            w: r.w - 2,
            h: DCTX_ITEM_H,
        }
    }

    fn hit_item(&self, mx: i32, my: i32, sw: usize, sh: usize) -> Option<usize> {
        for i in 0..DCTX_ITEMS {
            let ir = self.item_rect(i, sw, sh);
            if mx >= ir.x as i32
                && mx < (ir.x + ir.w) as i32
                && my >= ir.y as i32
                && my < (ir.y + ir.h) as i32
            {
                return Some(i);
            }
        }
        None
    }
}
