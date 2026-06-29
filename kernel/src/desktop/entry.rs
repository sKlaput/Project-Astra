// ── Numeric formatting ────────────────────────────────────────────────────────

fn fmt_hms(buf: &mut [u8; 24], h: u64, m: u64, s: u64) -> usize {
    let mut i = 0usize;
    fn pu(buf: &mut [u8; 24], i: &mut usize, n: u64) {
        if n >= 10 {
            buf[*i] = b'0' + (n / 10) as u8;
            *i += 1;
        }
        buf[*i] = b'0' + (n % 10) as u8;
        *i += 1;
    }
    pu(buf, &mut i, h);
    buf[i] = b':';
    i += 1;
    if m < 10 {
        buf[i] = b'0';
        i += 1;
    }
    pu(buf, &mut i, m);
    buf[i] = b':';
    i += 1;
    if s < 10 {
        buf[i] = b'0';
        i += 1;
    }
    pu(buf, &mut i, s);
    i
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run() -> ! {
    // Ensure the VFS is mounted before any app tries to use fs::lookup / fs::open.
    // The boot phases that call probe_vfs() may be skipped, so we guarantee it here.
    let _ = crate::fs::mount_root();

    let (sw, sh) = framebuffer::dimensions().unwrap_or((1280, 800));
    let mut desktop = Desktop::new(sw, sh);
    desktop.load_desktop_state();
    desktop.damage.mark_full();

    let mut cursor_moved = false;
    let mut ev_buf = [Event::KeyPress(Key::Unknown(0)); 16];

    loop {
        let count = input::poll_events(&mut ev_buf);

        // ── Coalesce mouse moves ───────────────────────────────────────────
        // Only the last MouseMove position in the batch matters; intermediate
        // positions just cause redundant hover recalculation and extra damage.
        // Find the index of the last MouseMove (if any).
        let last_mm: Option<usize> = (0..count)
            .rev()
            .find(|&i| matches!(ev_buf[i], Event::MouseMove(..)));

        for i in 0..count {
            match ev_buf[i] {
                Event::MouseMove(mx, my) => {
                    if Some(i) == last_mm {
                        // Only process the final move in the batch.
                        desktop.on_mouse_move(mx, my);
                        cursor_moved = true;
                    } else {
                        // Earlier move: update position only (no hover, no damage).
                        desktop.cursor_x = mx;
                        desktop.cursor_y = my;
                        cursor_moved = true;
                    }
                }
                Event::MouseButton(buttons) => {
                    let prev = desktop.prev_btn_state;
                    desktop.prev_btn_state = buttons;
                    let pressed = buttons & !prev;
                    let released = !buttons & prev;
                    let (mx, my) = (desktop.cursor_x, desktop.cursor_y);
                    if pressed & 1 != 0 {
                        desktop.on_button_press(mx, my);
                    }
                    if pressed & 2 != 0 {
                        desktop.on_right_button_press(mx, my);
                    }
                    if released & 1 != 0 {
                        desktop.on_button_release();
                    }
                }
                Event::MouseScroll(delta) => {
                    desktop.on_mouse_scroll(delta);
                }
                Event::KeyPress(key) => {
                    desktop.on_key(key);
                }
            }
        }

        let now = uptime_ms();

        // Poll virtio-net for incoming frames and dispatch them to the network stack
        crate::net::poll_and_dispatch();

        desktop.tick_live_windows(now);

        // Update window snapshot for SysMonitor
        {
            let mut tbl = WIN_TABLE.lock();
            tbl.count = 0;
            for win in &desktop.windows {
                if tbl.count >= WIN_SNAP_MAX {
                    break;
                }
                let mut snap = WinSnap::empty();
                let t = win.app.title().as_bytes();
                let tl = t.len().min(32);
                snap.title[..tl].copy_from_slice(&t[..tl]);
                snap.title_len = tl;
                let id = win.app.app_id().as_bytes();
                let il = id.len().min(16);
                snap.app_id[..il].copy_from_slice(&id[..il]);
                snap.id_len = il;
                snap.minimized = win.minimized;
                let idx = tbl.count;
                tbl.snaps[idx] = snap;
                tbl.count += 1;
            }
        }

        if !desktop.damage.is_empty() {
            desktop.present_damage();
            cursor_moved = false;
        } else if cursor_moved {
            desktop.cursor_move_fast();
            cursor_moved = false;
        } else {
            // Sleep until the next periodic frame is due, but wake immediately
            // on any input so hover/keypress latency stays near-zero.
            // Strategy: HLT once (yields until next interrupt — PIT at 100Hz,
            // PS/2 mouse IRQ, PS/2 keyboard IRQ), then poll the PS/2 FIFO.
            // If input is pending, break out and process it; otherwise HLT again
            // until the deadline or more input arrives.
            let wakeup_ms = desktop.next_wakeup_ms(now);
            let deadline_ticks = if wakeup_ms < u64::MAX {
                let hz = timer_hz() as u64;
                let sleep_ms = wakeup_ms.saturating_sub(now);
                timer_ticks().saturating_add(((sleep_ms * hz + 999) / 1000).max(1))
            } else {
                u64::MAX
            };
            loop {
                idle_once(); // HLT — wakes on any IRQ (PIT, PS/2 kbd, PS/2 mouse)
                             // Drain PS/2 FIFO immediately after waking — before tick check.
                             // This picks up any mouse/kbd data that arrived since last poll.
                crate::drivers::mouse::poll_aux_bytes();
                if crate::drivers::keyboard::scancode_count() > 0
                    || crate::drivers::mouse::has_pending_packets()
                    || timer_ticks() >= deadline_ticks
                {
                    break;
                }
            }
        }
    }
}
