// ── Helpers ───────────────────────────────────────────────────────────────────

fn fmt_cursor_pos(buf: &mut [u8; 64], line: usize, col: usize, total: usize) -> &str {
    let mut i = 0usize;
    macro_rules! w {
        ($s:expr) => {
            for b in $s {
                if i < buf.len() {
                    buf[i] = *b;
                    i += 1;
                }
            }
        };
    }
    w!(b"Ln ");
    i = write_uint64(buf, i, line as u64);
    w!(b"  Col ");
    i = write_uint64(buf, i, col as u64);
    w!(b"  /  ");
    i = write_uint64(buf, i, total as u64);
    w!(b" lines");
    core::str::from_utf8(&buf[..i]).unwrap_or("")
}

fn write_uint64(buf: &mut [u8; 64], mut i: usize, mut n: u64) -> usize {
    if n == 0 {
        if i < buf.len() {
            buf[i] = b'0';
            i += 1;
        }
        return i;
    }
    let start = i;
    while n > 0 && i < buf.len() {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    buf[start..i].reverse();
    i
}

fn fmt_lnum(buf: &mut [u8; 5], n: usize) {
    buf[4] = b' ';
    let digits = [
        b'0' + ((n / 1000) % 10) as u8,
        b'0' + ((n / 100) % 10) as u8,
        b'0' + ((n / 10) % 10) as u8,
        b'0' + (n % 10) as u8,
    ];
    let mut started = false;
    for i in 0..4 {
        if digits[i] != b'0' {
            started = true;
        }
        buf[i] = if started { digits[i] } else { b' ' };
    }
    if !started {
        buf[3] = b'0';
    }
}

fn fmt_line_info(buf: &mut [u8; 48], cur: usize, total: usize, pct: usize) -> &str {
    let mut i = 0usize;
    macro_rules! w {
        ($s:expr) => {
            for b in $s {
                if i < buf.len() {
                    buf[i] = *b;
                    i += 1;
                }
            }
        };
    }
    w!(b"Ln ");
    i = write_uint(buf, i, cur);
    w!(b" / ");
    i = write_uint(buf, i, total);
    w!(b"   (");
    i = write_uint(buf, i, pct);
    w!(b"%)");
    core::str::from_utf8(&buf[..i]).unwrap_or("")
}

fn write_uint(buf: &mut [u8; 48], mut i: usize, mut n: usize) -> usize {
    if n == 0 {
        if i < buf.len() {
            buf[i] = b'0';
            i += 1;
        }
        return i;
    }
    let start = i;
    while n > 0 && i < buf.len() {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    buf[start..i].reverse();
    i
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

/// Writes a shortened path into `buf`.  If the path fits within `max_chars` it
/// is copied verbatim; otherwise `...` is prepended and the trailing portion of
/// the path is kept so the filename/tail stays readable.
fn fmt_path_short(buf: &mut [u8; 64], path: &[u8], max_chars: usize) -> usize {
    let max = max_chars.min(64);
    if path.len() <= max {
        let n = path.len().min(64);
        buf[..n].copy_from_slice(&path[..n]);
        n
    } else if max > 3 {
        buf[0] = b'.';
        buf[1] = b'.';
        buf[2] = b'.';
        let skip = path.len().saturating_sub(max - 3);
        let copy = (path.len() - skip).min(61);
        buf[3..3 + copy].copy_from_slice(&path[skip..skip + copy]);
        3 + copy
    } else {
        0
    }
}

