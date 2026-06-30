fn fmt_u64(buf: &mut [u8], mut n: u64) -> usize {
    if buf.is_empty() {
        return 0;
    }
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut pos = tmp.len();
    while n > 0 {
        pos -= 1;
        tmp[pos] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let len = (tmp.len() - pos).min(buf.len());
    buf[..len].copy_from_slice(&tmp[pos..pos + len]);
    len
}

fn fmt_padded2(buf: &mut [u8], n: u64) -> usize {
    if buf.len() < 2 {
        return 0;
    }
    buf[0] = b'0' + ((n / 10) % 10) as u8;
    buf[1] = b'0' + (n % 10) as u8;
    2
}

fn fmt_uptime(buf: &mut [u8], hrs: u64, mins: u64, secs: u64) -> usize {
    let mut pos = 0;
    pos += fmt_u64(&mut buf[pos..], hrs);
    if pos < buf.len() {
        buf[pos] = b'h';
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b' ';
        pos += 1;
    }
    pos += fmt_padded2(&mut buf[pos..], mins);
    if pos < buf.len() {
        buf[pos] = b'm';
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b' ';
        pos += 1;
    }
    pos += fmt_padded2(&mut buf[pos..], secs);
    if pos < buf.len() {
        buf[pos] = b's';
        pos += 1;
    }
    pos
}

fn fmt_kb_of_kb(buf: &mut [u8], used: u64, total: u64) -> usize {
    let mut pos = 0;
    pos += fmt_u64(&mut buf[pos..], used);
    if pos + 4 <= buf.len() {
        buf[pos] = b' ';
        buf[pos + 1] = b'K';
        buf[pos + 2] = b'B';
        buf[pos + 3] = b' ';
        pos += 4;
    }
    if pos + 2 <= buf.len() {
        buf[pos] = b'/';
        buf[pos + 1] = b' ';
        pos += 2;
    }
    pos += fmt_u64(&mut buf[pos..], total);
    if pos + 3 <= buf.len() {
        buf[pos] = b' ';
        buf[pos + 1] = b'K';
        buf[pos + 2] = b'B';
        pos += 3;
    }
    pos
}
