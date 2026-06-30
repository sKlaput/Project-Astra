// ── Rendering helpers ─────────────────────────────────────────────────────────

fn draw_cell(cx: usize, cy: usize, col_idx: u8, ghost: bool) {
    if col_idx == 0 {
        return;
    }
    let bg = if ghost {
        GHOST_COL
    } else {
        PIECE_COLS[col_idx as usize]
    };
    let dk = if ghost {
        GHOST_COL
    } else {
        PIECE_DARK[col_idx as usize]
    };
    // Fill with bright color, then slightly darker inset
    framebuffer::fill_rect(cx, cy, CELL, CELL, bg);
    framebuffer::fill_rect(cx + 2, cy + 2, CELL - 4, CELL - 4, dk);
}

fn draw_mini_piece(piece: u8, px: usize, py: usize) {
    if piece == 0 {
        return;
    }
    const S: usize = 7; // mini-cell size
    let offs = PIECES[piece as usize][0];
    // Find bounding box
    let min_c = offs.iter().map(|&(c, _)| c).min().unwrap_or(0);
    let min_r = offs.iter().map(|&(_, r)| r).min().unwrap_or(0);
    let col = PIECE_COLS[piece as usize];
    let dk = PIECE_DARK[piece as usize];
    for (c, r) in offs {
        let x = px + (c - min_c) as usize * S;
        let y = py + (r - min_r) as usize * S;
        framebuffer::fill_rect(x, y, S, S, col);
        framebuffer::fill_rect(x + 1, y + 1, S - 2, S - 2, dk);
    }
}

fn draw_label(x: usize, y: usize, s: &str) {
    framebuffer::draw_text_at(x, y, s, LABEL_COL);
}

fn draw_value_u32(x: usize, y: usize, v: u32) {
    let mut buf = [0u8; 12];
    let mut i = 12usize;
    let mut n = v;
    if n == 0 {
        buf[11] = b'0';
        i = 11;
    }
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        framebuffer::draw_text_at(x, y, s, VALUE_COL);
    }
}

// ── App trait ─────────────────────────────────────────────────────────────────

