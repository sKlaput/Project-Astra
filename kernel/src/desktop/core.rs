// ── Desktop struct ────────────────────────────────────────────────────────────

struct Desktop {
    sw: usize,
    sh: usize,
    windows: Vec<Window>,
    focused: Option<usize>,
    app_hover_target: Option<usize>,
    cursor_x: i32,
    cursor_y: i32,
    cursor_shape: CursorShape,
    drag: Option<DragState>,
    resize: Option<ResizeState>,
    icon_drag: Option<IconDragState>,
    launcher_open: bool,
    launcher_hover: Option<usize>,
    icons: [DesktopIcon; NUM_ICONS],
    icon_hover: Option<usize>,
    taskbar_hover: Option<usize>,
    close_hover: Option<usize>,
    cascade_x: i32,
    cascade_y: i32,
    damage: DamageList,
    cursor_under: [u32; CURSOR_W * CURSOR_H],
    cursor_drawn_x: i32,
    cursor_drawn_y: i32,
    cursor_on_screen: bool,
    prev_btn_state: u8,
    dctx: DesktopCtxMenu,
    desk_items: [DesktopItem; MAX_DESK_ITEMS],
    desk_item_count: usize,
    desk_prompt: DesktopNamePrompt,
    desk_item_drag: Option<DeskItemDrag>,
    // ── Drag rate-limiter ─────────────────────────────────────────────────
    // Accumulates the union of all window positions (old + new) that have not
    // yet been composited.  Flushed to the damage list at most every 16 ms
    // (~60 fps cap) so QEMU's VGA emulation is not overwhelmed by 100 Hz
    // MMIO storms.  Reset to Some(latest_bounds) after each present so the
    // next frame correctly erases the last-drawn position.
    drag_damage_accum: Option<Rect>,
    last_drag_present_ms: u64,
}

impl Desktop {
    fn new(sw: usize, sh: usize) -> Self {
        Desktop {
            sw,
            sh,
            windows: Vec::new(),
            focused: None,
            app_hover_target: None,
            cursor_x: (sw / 2) as i32,
            cursor_y: (sh / 2) as i32,
            cursor_shape: CursorShape::Arrow,
            drag: None,
            resize: None,
            icon_drag: None,
            launcher_open: false,
            launcher_hover: None,
            icons: {
                let mk = |row: usize| {
                    let r = icon_rect(row, BAR_H);
                    DesktopIcon {
                        x: r.x as i32,
                        y: r.y as i32,
                        selected: false,
                        last_click_ms: 0,
                    }
                };
                [
                    mk(0),
                    mk(1),
                    mk(2),
                    mk(3),
                    mk(4),
                    mk(5),
                    mk(6),
                    mk(7),
                    mk(8),
                    mk(9),
                    mk(10),
                ]
            },
            icon_hover: None,
            taskbar_hover: None,
            close_hover: None,
            cascade_x: 120,
            cascade_y: BAR_H as i32 + 20,
            damage: DamageList::new(),
            cursor_under: [0u32; CURSOR_W * CURSOR_H],
            cursor_drawn_x: 0,
            cursor_drawn_y: 0,
            cursor_on_screen: false,
            prev_btn_state: 0,
            dctx: DesktopCtxMenu::hidden(),
            desk_items: [DesktopItem::blank(); MAX_DESK_ITEMS],
            desk_item_count: 0,
            desk_prompt: DesktopNamePrompt::hidden(),
            desk_item_drag: None,
            drag_damage_accum: None,
            last_drag_present_ms: 0,
        }
    }

    // ── Window management ─────────────────────────────────────────────────

    fn open_window(&mut self, app: Box<dyn App>) {
        // Singleton policy: if a window with this app_id is already open,
        // un-minimize it and bring it to the front instead of spawning again.
        if !app.allow_multiple_instances() {
            let id = app.app_id();
            for i in 0..self.windows.len() {
                if self.windows[i].app.app_id() == id {
                    if self.windows[i].minimized {
                        self.windows[i].minimized = false;
                    }
                    let b = self.windows[i].bounds();
                    self.damage.add(b);
                    self.raise_to_front(i);
                    self.focused = Some(self.windows.len() - 1);
                    self.damage
                        .add(self.windows[self.windows.len() - 1].bounds());
                    return;
                }
            }
        }
        if self.windows.len() >= MAX_WINDOWS {
            return;
        }
        let (pw, ph) = app.preferred_size();
        let w = pw.min(self.sw.saturating_sub(40));
        let h = ph.min(self.sh.saturating_sub(BAR_H + 40));

        // Smart cascade placement:
        // - Both axes wrap together (avoids corner pile-up).
        // - Nudge forward if proposed position would exactly overlap an existing
        //   window's title bar (within 8 px).
        const CASCADE_STEP: i32 = 28;
        const CASCADE_BASE_X: i32 = 120;
        const CASCADE_BASE_Y: i32 = BAR_H as i32 + 20;
        let max_x = (self.sw.saturating_sub(w + 20)) as i32;
        let max_y = (self.sh.saturating_sub(h + 20)) as i32;

        let mut cx = self.cascade_x;
        let mut cy = self.cascade_y;
        // Wrap both axes together when either hits the edge.
        if cx > max_x || cy > max_y {
            cx = CASCADE_BASE_X;
            cy = CASCADE_BASE_Y;
        }
        // Nudge until no existing window title-bar overlaps within 8 px.
        for _ in 0..MAX_WINDOWS {
            let overlap = self
                .windows
                .iter()
                .any(|win| !win.minimized && (win.x - cx).abs() < 8 && (win.y - cy).abs() < 8);
            if !overlap {
                break;
            }
            cx = (cx + CASCADE_STEP).min(max_x);
            cy = (cy + CASCADE_STEP).min(max_y);
        }

        self.cascade_x = cx + CASCADE_STEP;
        self.cascade_y = cy + CASCADE_STEP;

        let win = Window {
            x: cx,
            y: cy,
            w,
            h,
            minimized: false,
            last_refresh_ms: 0,
            app,
            cached_surface: Vec::new(),
            surface_valid: false,
            surface_w: 0,
            surface_h: 0,
            surface_needs_capture: false,
        };
        let b = win.bounds();
        self.windows.push(win);
        self.focused = Some(self.windows.len() - 1);
        self.damage.add(b);
    }

    fn close_window(&mut self, idx: usize) {
        if idx >= self.windows.len() {
            return;
        }
        // Give the app a chance to intercept (e.g. unsaved-changes prompt)
        let action = self.windows[idx].app.request_close();
        if action != crate::app::AppAction::Close {
            self.handle_app_action(idx, action);
            return;
        }
        let b = self.windows[idx].bounds();
        self.damage.add(b);
        self.windows.remove(idx);
        // Clear any drag/resize that was tracking the removed window.
        if matches!(&self.drag,   Some(d) if d.win_idx == idx) {
            self.drag = None;
        }
        if matches!(&self.resize, Some(r) if r.win_idx == idx) {
            self.resize = None;
        }
        if self.windows.is_empty() {
            self.focused = None;
        } else {
            self.focused = Some(self.windows.len() - 1);
        }
        self.damage.mark_full();
    }

    fn raise_to_front(&mut self, idx: usize) {
        if idx >= self.windows.len() || idx == self.windows.len() - 1 {
            return;
        }
        let win = self.windows.remove(idx);
        self.windows.push(win);
    }

    fn minimize_all(&mut self) {
        for w in &mut self.windows {
            w.minimized = true;
        }
        self.focused = None;
        self.damage.mark_full();
    }

    // ── Hit testing ───────────────────────────────────────────────────────

    fn window_at(&self, mx: i32, my: i32) -> Option<usize> {
        for i in (0..self.windows.len()).rev() {
            let w = &self.windows[i];
            if w.minimized {
                continue;
            }
            if mx >= w.x && mx < w.x + w.w as i32 && my >= w.y && my < w.y + w.h as i32 {
                return Some(i);
            }
        }
        None
    }

    fn icon_at(&self, mx: i32, my: i32) -> Option<usize> {
        if mx < 0 || my < 0 {
            return None;
        }
        for i in 0..NUM_ICONS {
            let r = icon_rect_of(&self.icons[i]);
            if mx as usize >= r.x
                && (mx as usize) < r.x + r.w
                && my as usize >= r.y
                && (my as usize) < r.y + r.h
            {
                return Some(i);
            }
        }
        None
    }

    fn launcher_item_at(&self, mx: i32, my: i32) -> Option<usize> {
        if !self.launcher_open {
            return None;
        }
        if mx < 0 || mx as usize >= LAUNCHER_W {
            return None;
        }
        let item_start_y = BAR_H + LAUNCHER_HEAD_H;
        let item_end_y = item_start_y + NUM_LAUNCHER * LAUNCHER_ITEM_H;
        if my as usize >= item_start_y && (my as usize) < item_end_y {
            Some(((my as usize) - item_start_y) / LAUNCHER_ITEM_H)
        } else {
            None
        }
    }

    fn taskbar_btn_at(&self, mx: i32, my: i32) -> Option<usize> {
        if my < 0 || my as usize >= BAR_H {
            return None;
        }
        let total = FIXED_BTNS + self.windows.iter().filter(|w| !w.minimized).count();
        for i in 0..total {
            let r = taskbar_btn_rect(i);
            if mx as usize >= r.x
                && (mx as usize) < r.x + r.w
                && my as usize >= r.y
                && (my as usize) < r.y + r.h
            {
                return Some(i);
            }
        }
        None
    }

}
