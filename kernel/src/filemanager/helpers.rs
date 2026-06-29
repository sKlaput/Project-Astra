// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse a VFS path into breadcrumb segments.
/// Returns (byte_start, byte_len) for each segment in `path`.
/// Segment 0 is always the root "/".
fn parse_crumbs(path: &[u8], out: &mut [(usize, usize); MAX_CRUMBS]) -> usize {
    out[0] = (0, 1); // root "/"
    let mut count = 1usize;
    let mut i = 1usize;
    while i < path.len() && count < MAX_CRUMBS {
        let start = i;
        while i < path.len() && path[i] != b'/' {
            i += 1;
        }
        if start < i {
            out[count] = (start, i - start);
            count += 1;
        }
        if i < path.len() {
            i += 1;
        } // skip '/'
    }
    count
}

fn truncate_str(s: &str, max: usize) -> &str {
    let b = s.as_bytes();
    if b.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && (b[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    core::str::from_utf8(&b[..end]).unwrap_or("")
}

fn fmt_uint(buf: &mut [u8; 16], pos: usize, mut n: usize) -> usize {
    if n == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
        }
        return pos + 1;
    }
    let start = pos;
    let mut i = pos;
    while n > 0 && i < buf.len() {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    buf[start..i].reverse();
    i
}

fn fmt_uint_u64(buf: &mut [u8; 24], pos: usize, mut n: u64) -> usize {
    if n == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
        }
        return pos + 1;
    }
    let start = pos;
    let mut i = pos;
    while n > 0 && i < buf.len() {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    buf[start..i].reverse();
    i
}

fn fmt_count(buf: &mut [u8; 20], n: usize) -> &str {
    let mut tmp = [0u8; 16];
    let end = {
        let mut i = 0usize;
        let mut v = n;
        if v == 0 {
            tmp[0] = b'0';
            i = 1;
        } else {
            while v > 0 && i < tmp.len() {
                tmp[i] = b'0' + (v % 10) as u8;
                v /= 10;
                i += 1;
            }
            tmp[..i].reverse();
        }
        i
    };
    let nstr = core::str::from_utf8(&tmp[..end]).unwrap_or("0");
    let mut i = 0usize;
    for b in nstr.bytes() {
        if i < buf.len() {
            buf[i] = b;
            i += 1;
        }
    }
    for b in b" items" {
        if i < buf.len() {
            buf[i] = *b;
            i += 1;
        }
    }
    core::str::from_utf8(&buf[..i]).unwrap_or("")
}

/// Encode a u16 as lowercase hex into a 4-byte array.
/// Returns (buf, len) where len is always 4.
fn hex_u16(v: u16) -> ([u8; 4], usize) {
    let hex = b"0123456789abcdef";
    let buf = [
        hex[((v >> 12) & 0xF) as usize],
        hex[((v >> 8) & 0xF) as usize],
        hex[((v >> 4) & 0xF) as usize],
        hex[(v & 0xF) as usize],
    ];
    (buf, 4)
}
