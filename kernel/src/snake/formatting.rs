// ── Number formatting helpers ─────────────────────────────────────────────────

fn fmt_score(buf: &mut [u8; 32], prefix: &[u8], n: u32) -> usize {
    let mut i = 0usize;
    for &b in prefix {
        buf[i] = b;
        i += 1;
    }
    let s = n.to_str_buf(buf, &mut i);
    let _ = s;
    i
}

trait ToStrBuf {
    fn to_str_buf(self, buf: &mut [u8; 32], i: &mut usize) -> usize;
}

impl ToStrBuf for u32 {
    fn to_str_buf(self, buf: &mut [u8; 32], i: &mut usize) -> usize {
        if self == 0 {
            buf[*i] = b'0';
            *i += 1;
            return *i;
        }
        let mut tmp = [0u8; 10];
        let mut ti = 0usize;
        let mut n = self;
        while n > 0 {
            tmp[ti] = b'0' + (n % 10) as u8;
            ti += 1;
            n /= 10;
        }
        for j in (0..ti).rev() {
            buf[*i] = tmp[j];
            *i += 1;
        }
        *i
    }
}

