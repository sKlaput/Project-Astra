fn steer(g: &mut Game, d: Dir) -> AppAction {
    // Start game on first move input
    if g.phase == Phase::Ready {
        g.phase = Phase::Playing;
        g.last_move_ms = uptime_ms();
    }
    if g.phase == Phase::Playing && d != g.dir.opposite() {
        g.next_dir = d;
    }
    AppAction::Nothing
}

fn draw_overlay(
    cx: usize,
    cy: usize,
    cw: usize,
    ch: usize,
    heading: &str,
    hcol: u32,
    body: &str,
    bcol: u32,
) {
    let ow = 280usize;
    let oh = 80usize;
    let ox = cx + (cw.saturating_sub(ow)) / 2;
    let oy = cy + (ch.saturating_sub(oh)) / 2;
    framebuffer::fill_rect(ox, oy, ow, oh, OVER_BG);
    framebuffer::fill_rect(ox, oy, ow, 2, BORDER);
    framebuffer::fill_rect(ox, oy + oh - 2, ow, 2, BORDER);
    framebuffer::fill_rect(ox, oy, 2, oh, BORDER);
    framebuffer::fill_rect(ox + ow - 2, oy, 2, oh, BORDER);

    let text_w = heading.len() * 12;
    let tx = ox + (ow.saturating_sub(text_w)) / 2;
    framebuffer::draw_text_scaled(tx, oy + 16, heading, hcol, 2);

    let bw = body.len() * 6;
    let bx = ox + (ow.saturating_sub(bw)) / 2;
    framebuffer::draw_text_at(bx, oy + 52, body, bcol);
}

