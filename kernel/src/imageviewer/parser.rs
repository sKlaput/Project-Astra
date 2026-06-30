// ── PPM P6 parser ─────────────────────────────────────────────────────────────

/// Parse a PPM P6 header and return `Some((width, height, pixel_data_offset))`.
fn parse_ppm_p6(data: &[u8]) -> Option<(usize, usize, usize)> {
    let mut pos = 0usize;

    // Magic "P6"
    if data.len() < 3 || &data[..2] != b"P6" {
        return None;
    }
    pos += 2;

    // Skip whitespace after magic
    pos = skip_ws(data, pos);
    if pos >= data.len() {
        return None;
    }

    // Width
    let (w, p2) = parse_uint(data, pos)?;
    pos = skip_ws(data, p2);

    // Height
    let (h, p3) = parse_uint(data, pos)?;
    pos = skip_ws(data, p3);

    // Max value (must be 255)
    let (maxval, p4) = parse_uint(data, pos)?;
    if maxval != 255 {
        return None;
    }
    pos = p4;

    // Single whitespace after max value (required by spec)
    if pos >= data.len() {
        return None;
    }
    pos += 1; // consume the single whitespace byte

    // Verify there's enough pixel data
    if w == 0 || h == 0 {
        return None;
    }
    let pixel_bytes = w * h * 3;
    if pos + pixel_bytes > data.len() {
        return None;
    }

    Some((w, h, pos))
}

fn skip_ws(data: &[u8], mut pos: usize) -> usize {
    while pos < data.len() {
        match data[pos] {
            b' ' | b'\t' | b'\r' | b'\n' => pos += 1,
            b'#' => {
                // Comment: skip to end of line
                while pos < data.len() && data[pos] != b'\n' {
                    pos += 1;
                }
            }
            _ => break,
        }
    }
    pos
}

fn parse_uint(data: &[u8], mut pos: usize) -> Option<(usize, usize)> {
    if pos >= data.len() || !data[pos].is_ascii_digit() {
        return None;
    }
    let mut n = 0usize;
    while pos < data.len() && data[pos].is_ascii_digit() {
        n = n * 10 + (data[pos] - b'0') as usize;
        pos += 1;
    }
    Some((n, pos))
}

