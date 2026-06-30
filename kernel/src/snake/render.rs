// ── Rendering ─────────────────────────────────────────────────────────────────

fn draw_cell(gx: usize, gy: usize, cx: usize, cy: usize, color: u32) {
    let px = cx + X_OFF + gx * CELL;
    let py = cy + Y_OFF + gy * CELL;
    framebuffer::fill_rect(px + 1, py + 1, CELL - 2, CELL - 2, color);
}

