fn fmt_usize(buf: &mut [u8; 8], mut n: usize) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 8];
    let mut ti = 0usize;
    while n > 0 {
        tmp[ti] = b'0' + (n % 10) as u8;
        ti += 1;
        n /= 10;
    }
    for i in 0..ti {
        buf[i] = tmp[ti - 1 - i];
    }
    ti
}

fn write_label(buf: &mut [u8; 80], pos: &mut usize, label: &[u8]) {
    for &b in label {
        if *pos < buf.len() {
            buf[*pos] = b;
            *pos += 1;
        }
    }
}

fn write_usize_s(buf: &mut [u8; 80], pos: &mut usize, mut n: usize) {
    if n == 0 {
        if *pos < buf.len() {
            buf[*pos] = b'0';
            *pos += 1;
        }
        return;
    }
    let start = *pos;
    let mut tmp = [0u8; 20];
    let mut ti = 0usize;
    while n > 0 {
        tmp[ti] = b'0' + (n % 10) as u8;
        ti += 1;
        n /= 10;
    }
    let end = start + ti;
    if end > buf.len() {
        return;
    }
    for i in 0..ti {
        buf[start + i] = tmp[ti - 1 - i];
    }
    *pos = end;
}
