fn fixed_from_str(s: &[u8]) -> Option<Fixed> {
    // Accept optional leading '-', digits, optional '.', digits.
    if s.is_empty() {
        return None;
    }
    let (neg, s) = if s[0] == b'-' {
        (true, &s[1..])
    } else {
        (false, s)
    };
    let dot = s.iter().position(|&b| b == b'.');
    let int_part = if let Some(d) = dot { &s[..d] } else { s };
    let frac_part = if let Some(d) = dot {
        &s[d + 1..]
    } else {
        b"" as &[u8]
    };

    let mut v: i64 = 0;
    for &b in int_part {
        if b < b'0' || b > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as i64)?;
    }
    v = v.checked_mul(SCALE)?;

    let mut frac_scale = SCALE / 10;
    for &b in frac_part {
        if b < b'0' || b > b'9' {
            return None;
        }
        if frac_scale > 0 {
            v = v.checked_add((b - b'0') as i64 * frac_scale)?;
            frac_scale /= 10;
        }
    }

    Some(if neg { -v } else { v })
}

fn fixed_to_str(buf: &mut [u8; 32], v: Fixed) -> usize {
    if v == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut pos = 0usize;
    let neg = v < 0;
    let mut abs = if neg { v.wrapping_neg() } else { v };
    // Clamp to avoid UB if i64::MIN
    if abs < 0 {
        abs = i64::MAX;
    }

    let int_part = abs / SCALE;
    let frac_part = abs % SCALE;

    if neg {
        buf[pos] = b'-';
        pos += 1;
    }

    // Integer digits
    let int_start = pos;
    let mut tmp = int_part;
    if tmp == 0 {
        buf[pos] = b'0';
        pos += 1;
    } else {
        let digit_start = pos;
        while tmp > 0 {
            buf[pos] = b'0' + (tmp % 10) as u8;
            pos += 1;
            tmp /= 10;
        }
        // Reverse the integer digits
        buf[int_start..pos].reverse();
        let _ = digit_start;
    }

    // Fractional part — trim trailing zeros, up to 6 places
    if frac_part != 0 {
        buf[pos] = b'.';
        pos += 1;
        let mut fp = frac_part;
        let mut frac_digits = [0u8; 6];
        for i in (0..6).rev() {
            frac_digits[i] = (fp % 10) as u8;
            fp /= 10;
        }
        // Trim trailing zeros
        let mut end = 6;
        while end > 0 && frac_digits[end - 1] == 0 {
            end -= 1;
        }
        for &d in &frac_digits[..end] {
            if pos < 32 {
                buf[pos] = b'0' + d;
                pos += 1;
            }
        }
    }
    pos
}

fn fixed_div(a: Fixed, b: Fixed) -> Option<Fixed> {
    if b == 0 {
        return None;
    }
    // a/b as fixed = (a * SCALE) / b — but watch for overflow
    // Use i128 intermediate
    let result = (a as i128 * SCALE as i128) / b as i128;
    if result > i64::MAX as i128 || result < i64::MIN as i128 {
        None
    } else {
        Some(result as i64)
    }
}

fn fixed_mul(a: Fixed, b: Fixed) -> Option<Fixed> {
    let result = (a as i128 * b as i128) / SCALE as i128;
    if result > i64::MAX as i128 || result < i64::MIN as i128 {
        None
    } else {
        Some(result as i64)
    }
}

fn apply_op(a: Fixed, op: char, b: Fixed) -> Option<Fixed> {
    match op {
        '+' => a.checked_add(b),
        '-' => a.checked_sub(b),
        '*' => fixed_mul(a, b),
        '/' => fixed_div(a, b),
        _ => None,
    }
}

