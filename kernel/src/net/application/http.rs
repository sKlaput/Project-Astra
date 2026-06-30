// ---------------------------------------------------------------------------
// Astra OS — HTTP/1.0 GET client
//
// Sends a single HTTP/1.0 GET request over TCP and reads the response.
// HTTP/1.0 has no keep-alive so the server closes after the response —
// making it easy to detect end of response (TCP FIN or timeout).
//
// Usage:
//   let result = http::get("example.com", 80, "/", &mut buf);
//   // result = Ok(bytes_in_buf) or Err(HttpError)
// ---------------------------------------------------------------------------

use crate::arch::x86_64::interrupts::uptime_ms;
use crate::net::{config, dns, tcp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpError {
    NicNotReady,
    DnsTimeout,
    ConnectTimeout,
    SendFailed,
    ResponseTimeout,
    BufferTooSmall,
}

pub const DEFAULT_TIMEOUT_MS: u64 = 5000;

/// Perform an HTTP/1.0 GET request.
///
/// `host`    — hostname or dotted-decimal IP string.
/// `port`    — TCP port (usually 80).
/// `path`    — request path (e.g. "/", "/index.html").
/// `out`     — buffer to write the raw HTTP response into.
///
/// Returns `Ok(n)` where `n` is the number of bytes written to `out`.
pub fn get(host: &str, port: u16, path: &str, out: &mut [u8]) -> Result<usize, HttpError> {
    if !crate::net::driver::is_ready() {
        return Err(HttpError::NicNotReady);
    }
    config::get().ok_or(HttpError::NicNotReady)?;

    // ── Resolve IP ────────────────────────────────────────────────────────
    let dst_ip = if let Some(ip) = parse_dotted_ip(host) {
        ip
    } else {
        dns::resolve(host, 3000).map_err(|_| HttpError::DnsTimeout)?
    };

    // ── TCP connect ───────────────────────────────────────────────────────
    if !tcp::connect(dst_ip, port, DEFAULT_TIMEOUT_MS) {
        return Err(HttpError::ConnectTimeout);
    }

    // ── Build HTTP/1.0 GET request ─────────────────────────────────────────
    // HTTP/1.0 — server closes connection after response, no keep-alive needed.
    const REQ_CAP: usize = 512;
    let mut req = [0u8; REQ_CAP];
    let mut p = 0usize;

    let append = |buf: &mut [u8; REQ_CAP], pos: &mut usize, s: &str| {
        let b = s.as_bytes();
        let n = b.len().min(REQ_CAP - *pos);
        buf[*pos..*pos + n].copy_from_slice(&b[..n]);
        *pos += n;
    };

    append(&mut req, &mut p, "GET ");
    append(&mut req, &mut p, path);
    append(&mut req, &mut p, " HTTP/1.0\r\nHost: ");
    append(&mut req, &mut p, host);
    append(&mut req, &mut p, "\r\nConnection: close\r\n\r\n");

    if !tcp::send(&req[..p]) {
        tcp::close();
        return Err(HttpError::SendFailed);
    }

    // ── Read response ─────────────────────────────────────────────────────
    let mut total = 0usize;
    let deadline = uptime_ms() + DEFAULT_TIMEOUT_MS;
    let mut timed_out = false;
    // Spin counter: poll several times before halting to avoid 10ms hlt
    // penalty between consecutive TCP segments from the server.
    let mut spin = 0u32;

    loop {
        crate::net::poll_and_dispatch();

        // Drain whatever arrived into our output buffer
        if tcp::has_data() {
            if total >= out.len() {
                tcp::close();
                return Err(HttpError::BufferTooSmall);
            }
            let n = tcp::read(&mut out[total..]);
            total += n;
            spin = 0; // reset spin counter on progress
        }

        if tcp::is_closed_remote() {
            break;
        }

        let s = tcp::state();
        if s == tcp::TcpState::Closed || s == tcp::TcpState::TimeWait {
            break;
        }

        if uptime_ms() >= deadline {
            timed_out = true;
            break;
        }

        // Spin a few rounds before halting so back-to-back TCP segments
        // from the server are caught without waiting for the next timer IRQ.
        spin += 1;
        if spin >= 16 {
            spin = 0;
            crate::arch::x86_64::halt::idle_once();
        } else {
            core::hint::spin_loop();
        }
    }

    tcp::close();
    if timed_out && total == 0 {
        return Err(HttpError::ResponseTimeout);
    }
    Ok(total)
}

/// Try to parse `s` as a dotted-decimal IPv4 address.
fn parse_dotted_ip(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut idx = 0usize;
    let mut cur: u16 = 0;
    let mut digits = 0usize;
    for b in s.bytes() {
        match b {
            b'0'..=b'9' => {
                cur = cur * 10 + (b - b'0') as u16;
                if cur > 255 {
                    return None;
                }
                digits += 1;
            }
            b'.' => {
                if digits == 0 || idx >= 3 {
                    return None;
                }
                octets[idx] = cur as u8;
                idx += 1;
                cur = 0;
                digits = 0;
            }
            _ => return None,
        }
    }
    if idx != 3 || digits == 0 {
        return None;
    }
    octets[3] = cur as u8;
    Some(octets)
}
