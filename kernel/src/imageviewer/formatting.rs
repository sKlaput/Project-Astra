// ── Formatting helper ─────────────────────────────────────────────────────────

fn write_usize(buf: &mut [u8], pos: &mut usize, mut n: usize) {
    let start = *pos;
    if n == 0 {
        if *pos < buf.len() {
            buf[*pos] = b'0';
            *pos += 1;
        }
        return;
    }
    let mut tmp = [0u8; 20];
    let mut ti = 0usize;
    while n > 0 {
        tmp[ti] = b'0' + (n % 10) as u8;
        ti += 1;
        n /= 10;
    }
    let end = *pos + ti;
    if end > buf.len() {
        return;
    }
    for i in 0..ti {
        buf[start + i] = tmp[ti - 1 - i];
    }
    *pos = end;
}

