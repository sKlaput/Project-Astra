fn render_system(cx: usize, cy: usize, cw: usize, _ch: usize, sel: usize) {
    let x = cx + PAD;
    let mut y = cy + PAD;
    framebuffer::draw_text_scaled(x, y, "System Information", HEADING, SC);
    y += CH + 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    let vx = x + 16 * CW;
    for (i, item) in SYSINFO.iter().enumerate() {
        if i == sel {
            framebuffer::fill_rect(cx + 2, y - 2, cw.saturating_sub(4), CH + 4, TAB_SEL_BG);
            framebuffer::draw_text_scaled(x, y, item.k, TAB_SEL_TXT, SC);
            framebuffer::draw_text_scaled(vx, y, item.v, TAB_SEL_TXT, SC);
        } else {
            framebuffer::draw_text_scaled(x, y, item.k, LABEL, SC);
            framebuffer::draw_text_scaled(vx, y, item.v, VALUE, SC);
        }
        y += CH + 4;
    }
    y += 8;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    framebuffer::draw_text_scaled(x, y, "Runtime", HEADING, SC);
    y += CH + 4;
    let ms = crate::arch::x86_64::interrupts::uptime_ms();
    let secs = ms / 1000;
    let mins = secs / 60;
    let hrs = mins / 60;
    let mut buf = [0u8; 32];
    let len = fmt_uptime(&mut buf, hrs, mins % 60, secs % 60);
    let s = unsafe { core::str::from_utf8_unchecked(&buf[..len]) };
    framebuffer::draw_text_scaled(x, y, "Uptime", LABEL, SC);
    framebuffer::draw_text_scaled(vx, y, s, VALUE, SC);
    y += CH + 4;
    let heap = crate::memory::heap::get_telemetry();
    let used_kb = (heap.used_bytes / 1024) as u64;
    let total_kb = ((heap.mapped_pages * 4096) / 1024) as u64;
    let mut buf2 = [0u8; 32];
    let len2 = fmt_kb_of_kb(&mut buf2, used_kb, total_kb);
    let s2 = unsafe { core::str::from_utf8_unchecked(&buf2[..len2]) };
    framebuffer::draw_text_scaled(x, y, "Heap", LABEL, SC);
    framebuffer::draw_text_scaled(vx, y, s2, VALUE, SC);
    let _ = y;
}

fn render_display(cx: usize, cy: usize, cw: usize, _ch: usize, sel: usize) {
    let x = cx + PAD;
    let mut y = cy + PAD;
    framebuffer::draw_text_scaled(x, y, "Desktop Background", HEADING, SC);
    y += CH + 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    framebuffer::draw_text_at(
        x,
        y,
        "Arrow keys to browse presets, Enter/Space to apply.",
        LABEL,
    );
    y += 14;

    // Current colour preview
    let cur_bg = crate::desktop::DESKTOP_BG_COLOR.load(AO::Relaxed);
    framebuffer::draw_text_at(x, y, "Current:", LABEL);
    framebuffer::fill_rect(x + 52, y - 1, 32, 11, 0x3A5878);
    framebuffer::fill_rect(x + 53, y, 30, 10, cur_bg);
    y += 20;

    // 2×4 swatch grid
    const SW: usize = 40;
    const SG: usize = 12;
    const COLS: usize = 4;

    for (i, (col, name)) in THEMES.iter().enumerate() {
        let is_sel = i == sel;
        let is_active = *col == cur_bg;
        let sx = x + (i % COLS) * (SW + SG);
        let sy = y + (i / COLS) * (SW + 22);
        // Border
        let border_col = if is_sel {
            SWATCH_SEL
        } else if is_active {
            ACCENT
        } else {
            0x2A3A4A
        };
        framebuffer::fill_rect(
            sx.saturating_sub(2),
            sy.saturating_sub(2),
            SW + 4,
            SW + 4,
            border_col,
        );
        framebuffer::fill_rect(sx, sy, SW, SW, *col);
        // Tiny inner border for very dark swatches
        framebuffer::fill_rect(sx, sy, SW, 1, 0x1A2A3A);
        framebuffer::fill_rect(sx, sy, 1, SW, 0x1A2A3A);
        let tc = if is_sel { VALUE } else { LABEL };
        framebuffer::draw_text_at(sx, sy + SW + 3, name, tc);
    }
}

fn render_input(cx: usize, cy: usize, cw: usize, _ch: usize, sel: usize) {
    let x = cx + PAD;
    let mut y = cy + PAD;
    framebuffer::draw_text_scaled(x, y, "Input Devices", HEADING, SC);
    y += CH + 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    const ITEMS: [(&str, &str); 2] = [("Keyboard", "PS/2 (IRQ1)"), ("Mouse", "PS/2 Aux (IRQ12)")];
    let vx = x + 14 * CW;
    for (i, (k, v)) in ITEMS.iter().enumerate() {
        if i == sel {
            framebuffer::fill_rect(cx + 2, y - 2, cw.saturating_sub(4), CH + 4, TAB_SEL_BG);
            framebuffer::draw_text_scaled(x, y, k, TAB_SEL_TXT, SC);
            framebuffer::draw_text_scaled(vx, y, v, TAB_SEL_TXT, SC);
        } else {
            framebuffer::draw_text_scaled(x, y, k, LABEL, SC);
            framebuffer::draw_text_scaled(vx, y, v, VALUE, SC);
        }
        y += CH + 4;
    }
    y += 10;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    framebuffer::draw_text_at(x, y, "Mouse sensitivity and key repeat coming soon.", HINT);
    let _ = y;
}

fn render_about(cx: usize, cy: usize, cw: usize, _ch: usize) {
    let x = cx + PAD;
    let mut y = cy + PAD;
    framebuffer::draw_text_scaled(x, y, "About Astra OS", HEADING, SC);
    y += CH + 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    const LINES: &[(&str, u32)] = &[
        ("Astra OS is a from-scratch Rust-first desktop", 0xE8F4FD),
        ("operating system prototype focused on control,", 0xE8F4FD),
        ("privacy, simplicity, and eventually gaming-", 0xE8F4FD),
        ("capable personal computing.", 0xE8F4FD),
        ("", 0),
        ("Written entirely in Rust (no_std, bare-metal).", 0xB0D4B8),
        ("x86_64 / Limine UEFI boot.", 0xB0D4B8),
        ("Virtio-blk + FAT32 persistent storage.", 0xB0D4B8),
        ("Virtio-net Ethernet driver.", 0xB0D4B8),
        ("Ring-3 ELF user processes via SYSCALL.", 0xB0D4B8),
    ];
    for (line, col) in LINES {
        if !line.is_empty() {
            framebuffer::draw_text_at(x, y, line, *col);
        }
        y += 13;
    }
    y += 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    framebuffer::draw_text_scaled(x, y, "Runtime", HEADING, SC);
    y += CH + 4;
    let ms = crate::arch::x86_64::interrupts::uptime_ms();
    let secs = ms / 1000;
    let mins = secs / 60;
    let hrs = mins / 60;
    let mut buf = [0u8; 32];
    let len = fmt_uptime(&mut buf, hrs, mins % 60, secs % 60);
    let s = unsafe { core::str::from_utf8_unchecked(&buf[..len]) };
    framebuffer::draw_text_at(x, y, "Uptime:", LABEL);
    framebuffer::draw_text_at(x + 7 * 6 + 4, y, s, VALUE);
    y += 14;
    let heap = crate::memory::heap::get_telemetry();
    let used_kb = (heap.used_bytes / 1024) as u64;
    let total_kb = ((heap.mapped_pages * 4096) / 1024) as u64;
    let mut buf2 = [0u8; 32];
    let len2 = fmt_kb_of_kb(&mut buf2, used_kb, total_kb);
    let s2 = unsafe { core::str::from_utf8_unchecked(&buf2[..len2]) };
    framebuffer::draw_text_at(x, y, "Heap:", LABEL);
    framebuffer::draw_text_at(x + 7 * 6 + 4, y, s2, VALUE);
    let _ = y;
}
