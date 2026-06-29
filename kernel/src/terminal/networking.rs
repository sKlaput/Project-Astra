fn cmd_net() {
    let (ready, link, tx, rx) = crate::net::driver::stats();
    let mut t = TERM.lock();
    if !ready {
        t.push_str("NIC: not present (no virtio-net device)", ERR_COL);
        return;
    }
    if link {
        t.push_str("NIC: virtio-net  link UP", 0x66FF66);
    } else {
        t.push_str("NIC: virtio-net  link DOWN", ERR_COL);
    }
    let mac = crate::net::driver::mac_addr();
    let mut mac_buf = [0u8; 24];
    let mut pos = 0usize;
    let pfx = b"MAC: ";
    mac_buf[..pfx.len()].copy_from_slice(pfx);
    pos += pfx.len();
    const HEX: &[u8] = b"0123456789abcdef";
    for i in 0..6 {
        if i > 0 {
            mac_buf[pos] = b':';
            pos += 1;
        }
        mac_buf[pos] = HEX[(mac[i] >> 4) as usize];
        pos += 1;
        mac_buf[pos] = HEX[(mac[i] & 0xF) as usize];
        pos += 1;
    }
    let s = unsafe { core::str::from_utf8_unchecked(&mac_buf[..pos]) };
    t.push_str(s, TEXT_NORM);
    let mut buf = [0u8; LINE_BUF];
    let mut p = 0usize;
    let pfx2 = b"TX frames: ";
    buf[..pfx2.len()].copy_from_slice(pfx2);
    p += pfx2.len();
    p += write_dec(&mut buf[p..], tx);
    let s2 = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
    t.push_str(s2, TEXT_NORM);
    let mut buf3 = [0u8; LINE_BUF];
    let mut p3 = 0usize;
    let pfx3 = b"RX frames: ";
    buf3[..pfx3.len()].copy_from_slice(pfx3);
    p3 += pfx3.len();
    p3 += write_dec(&mut buf3[p3..], rx);
    let s3 = unsafe { core::str::from_utf8_unchecked(&buf3[..p3]) };
    t.push_str(s3, TEXT_NORM);

    // Show IP config if available
    if let Some(cfg) = crate::net::config::get() {
        let gw = cfg.gateway;
        let mut ibuf = [0u8; LINE_BUF];
        let pfx_ip = b"IP:  ";
        let mut pp = pfx_ip.len().min(LINE_BUF);
        ibuf[..pp].copy_from_slice(&pfx_ip[..pp]);
        let ip_arr = [cfg.ip[0], cfg.ip[1], cfg.ip[2], cfg.ip[3]];
        pp += write_ipv4(&mut ibuf[pp..], ip_arr);
        let s4 = unsafe { core::str::from_utf8_unchecked(&ibuf[..pp]) };
        t.push_str(s4, TEXT_NORM);
        let mut gbuf = [0u8; LINE_BUF];
        let pfx_gw = b"GW:  ";
        let mut gp = pfx_gw.len().min(LINE_BUF);
        gbuf[..gp].copy_from_slice(&pfx_gw[..gp]);
        gp += write_ipv4(&mut gbuf[gp..], gw);
        let s5 = unsafe { core::str::from_utf8_unchecked(&gbuf[..gp]) };
        t.push_str(s5, TEXT_NORM);

        // Show RX and TX queue debug state
        let (rx_last, rx_hw) = crate::net::driver::debug_rx_state();
        let (tx_last, tx_hw) = crate::net::driver::debug_tx_state();
        let mut dbuf = [0u8; LINE_BUF];
        let mut dp = 0usize;
        let dpfx = b"RX q: sw=";
        let dl = dpfx.len().min(LINE_BUF);
        dbuf[..dl].copy_from_slice(&dpfx[..dl]);
        dp += dl;
        dp += write_dec(&mut dbuf[dp..], rx_last as u64);
        if dp + 4 < LINE_BUF {
            dbuf[dp..dp + 4].copy_from_slice(b" hw=");
            dp += 4;
        }
        dp += write_dec(&mut dbuf[dp..], rx_hw as u64);
        t.push_str(
            unsafe { core::str::from_utf8_unchecked(&dbuf[..dp]) },
            if rx_hw != rx_last {
                0x66FF66
            } else {
                TEXT_NORM
            },
        );
        let mut dbuf2 = [0u8; LINE_BUF];
        let mut dp2 = 0usize;
        let dpfx2 = b"TX q: sw=";
        let dl2 = dpfx2.len().min(LINE_BUF);
        dbuf2[..dl2].copy_from_slice(&dpfx2[..dl2]);
        dp2 += dl2;
        dp2 += write_dec(&mut dbuf2[dp2..], tx_last as u64);
        if dp2 + 4 < LINE_BUF {
            dbuf2[dp2..dp2 + 4].copy_from_slice(b" hw=");
            dp2 += 4;
        }
        dp2 += write_dec(&mut dbuf2[dp2..], tx_hw as u64);
        t.push_str(
            unsafe { core::str::from_utf8_unchecked(&dbuf2[..dp2]) },
            if tx_hw != tx_last { 0x66FF66 } else { 0xFFAA44 },
        );
    } else {
        t.push_str("IP: not configured", ERR_COL);
    }
}

/// Parse an IPv4 dotted-decimal string into `[u8; 4]`.  Returns None on failure.
fn parse_ip(s: &str) -> Option<[u8; 4]> {
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

/// Write `ip` as dotted-decimal into `buf`.  Returns bytes written.
fn write_ipv4(buf: &mut [u8], ip: [u8; 4]) -> usize {
    let mut pos = 0usize;
    for (i, &octet) in ip.iter().enumerate() {
        if i > 0 {
            if pos < buf.len() {
                buf[pos] = b'.';
                pos += 1;
            }
        }
        pos += write_dec(&mut buf[pos..], octet as u64);
    }
    pos
}

/// `ping <ip>` — send 4 ICMP echo requests and report RTT.
fn cmd_ping(args: &str) {
    let target = args.trim();
    if target.is_empty() {
        TERM.lock()
            .push_str("Usage: ping <ip>  e.g. ping 10.0.2.2", ERR_COL);
        return;
    }
    let dst = match parse_ip(target) {
        Some(ip) => ip,
        None => {
            TERM.lock().push_str("ping: invalid IP address", ERR_COL);
            return;
        }
    };

    if !crate::net::driver::is_ready() {
        TERM.lock().push_str("ping: NIC not ready", ERR_COL);
        return;
    }
    if crate::net::config::get().is_none() {
        TERM.lock().push_str("ping: IP not configured", ERR_COL);
        return;
    }

    {
        let mut t = TERM.lock();
        let mut buf = [0u8; LINE_BUF];
        let pfx = b"PING ";
        let mut p = pfx.len();
        buf[..p].copy_from_slice(pfx);
        p += write_ipv4(&mut buf[p..], dst);
        let sfx = b": 32 bytes data";
        let sl = sfx.len().min(LINE_BUF - p);
        buf[p..p + sl].copy_from_slice(&sfx[..sl]);
        p += sl;
        t.push_str(
            unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
            TEXT_NORM,
        );
    }

    // ── Phase 1: ARP resolution ────────────────────────────────────────────
    // Always resolve the target MAC before sending ICMP so we use a unicast
    // destination instead of falling back to broadcast (slirp drops broadcast ICMP).
    let dst_mac = resolve_arp(dst);
    let dst_mac = match dst_mac {
        Some(m) => {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"ARP  ";
            let mut p = pfx.len();
            buf[..p].copy_from_slice(pfx);
            p += write_ipv4(&mut buf[p..], dst);
            let sfx = b" -> ";
            let sl = sfx.len().min(LINE_BUF - p);
            buf[p..p + sl].copy_from_slice(&sfx[..sl]);
            p += sl;
            p += fmt_mac(&mut buf[p..], m);
            TERM.lock().push_str(
                unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
                0x88CCFF,
            );
            m
        }
        None => {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"ARP timeout for ";
            let mut p = pfx.len();
            buf[..p].copy_from_slice(pfx);
            p += write_ipv4(&mut buf[p..], dst);
            TERM.lock().push_str(
                unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
                ERR_COL,
            );
            TERM.lock().push_str(
                "ping: host unreachable (no ARP reply — check NIC RX)",
                ERR_COL,
            );
            return;
        }
    };

    // ── Phase 2: ICMP echo loop ────────────────────────────────────────────
    const COUNT: u16 = 4;
    const ID: u16 = 0xA57A;
    const WAIT_MS: u64 = 1500;

    for seq in 0..COUNT {
        crate::net::icmp::send_echo_request_to(dst, dst_mac, ID, seq);

        let deadline = crate::arch::x86_64::interrupts::uptime_ms() + WAIT_MS;
        let mut got_reply = false;
        while crate::arch::x86_64::interrupts::uptime_ms() < deadline {
            crate::net::poll_and_dispatch();
            if let Some(reply) = crate::net::icmp::poll_reply() {
                if reply.id == ID && reply.seq == seq {
                    let mut buf = [0u8; LINE_BUF];
                    let pfx = b"Reply from ";
                    let mut p = pfx.len();
                    buf[..p].copy_from_slice(pfx);
                    p += write_ipv4(&mut buf[p..], reply.from);
                    let sfx = b": seq=";
                    let sl = sfx.len().min(LINE_BUF - p);
                    buf[p..p + sl].copy_from_slice(&sfx[..sl]);
                    p += sl;
                    p += write_dec(&mut buf[p..], seq as u64);
                    let sfx2 = b" time=";
                    let sl2 = sfx2.len().min(LINE_BUF - p);
                    buf[p..p + sl2].copy_from_slice(&sfx2[..sl2]);
                    p += sl2;
                    p += write_dec(&mut buf[p..], reply.rtt_ms as u64);
                    if p < LINE_BUF {
                        buf[p] = b'm';
                        p += 1;
                    }
                    if p < LINE_BUF {
                        buf[p] = b's';
                        p += 1;
                    }
                    TERM.lock().push_str(
                        unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
                        0x66FF66,
                    );
                    got_reply = true;
                    break;
                }
            }
            crate::arch::x86_64::halt::idle_once();
        }

        if !got_reply {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"Request timeout  seq=";
            let mut p = pfx.len();
            buf[..p].copy_from_slice(pfx);
            p += write_dec(&mut buf[p..], seq as u64);
            TERM.lock().push_str(
                unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
                ERR_COL,
            );
        }
    }
}

/// Send ARP requests until the target MAC is in the cache, or 1000ms elapses.
fn resolve_arp(ip: [u8; 4]) -> Option<[u8; 6]> {
    crate::net::arp::resolve_with_retry(ip, 1050, 3)
}

/// Format a MAC address into buf as "xx:xx:xx:xx:xx:xx". Returns bytes written.
fn fmt_mac(buf: &mut [u8], mac: [u8; 6]) -> usize {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut p = 0usize;
    for i in 0..6 {
        if i > 0 && p < buf.len() {
            buf[p] = b':';
            p += 1;
        }
        if p < buf.len() {
            buf[p] = HEX[(mac[i] >> 4) as usize];
            p += 1;
        }
        if p < buf.len() {
            buf[p] = HEX[(mac[i] & 0xF) as usize];
            p += 1;
        }
    }
    p
}

/// `dns <hostname>` — resolve a hostname to an IPv4 address via QEMU's DNS at 10.0.2.3.
fn cmd_dns(args: &str) {
    let name = args.trim();
    if name.is_empty() {
        TERM.lock()
            .push_str("Usage: dns <hostname>  e.g. dns google.com", ERR_COL);
        return;
    }
    if !crate::net::driver::is_ready() {
        TERM.lock().push_str("dns: NIC not ready", ERR_COL);
        return;
    }

    {
        let mut t = TERM.lock();
        let mut buf = [0u8; LINE_BUF];
        let pfx = b"Resolving ";
        let mut p = pfx.len().min(LINE_BUF);
        buf[..p].copy_from_slice(&pfx[..p]);
        let nb = name.as_bytes();
        let nl = nb.len().min(LINE_BUF - p);
        buf[p..p + nl].copy_from_slice(&nb[..nl]);
        p += nl;
        if p < LINE_BUF {
            buf[p] = b'.';
            p += 1;
        }
        if p < LINE_BUF {
            buf[p] = b'.';
            p += 1;
        }
        if p < LINE_BUF {
            buf[p] = b'.';
            p += 1;
        }
        t.push_str(
            unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
            TEXT_NORM,
        );
    }

    match crate::net::dns::resolve(name, 3000) {
        Ok(ip) => {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"  -> ";
            let mut p = pfx.len();
            buf[..p].copy_from_slice(pfx);
            p += write_ipv4(&mut buf[p..], ip);
            TERM.lock().push_str(
                unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
                0x66FF66,
            );
        }
        Err(crate::net::dns::DnsError::ArpFailed) => {
            TERM.lock().push_str(
                "dns: gateway ARP failed (NIC or slirp unreachable)",
                ERR_COL,
            );
        }
        Err(crate::net::dns::DnsError::SendFailed) => {
            TERM.lock()
                .push_str("dns: UDP send failed (NIC TX error)", ERR_COL);
        }
        Err(crate::net::dns::DnsError::NxDomain) => {
            TERM.lock()
                .push_str("dns: NXDOMAIN (name does not exist)", ERR_COL);
        }
        Err(crate::net::dns::DnsError::RcodeError(rc)) => {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"dns: server error RCODE=";
            let mut p = pfx.len().min(LINE_BUF);
            buf[..p].copy_from_slice(&pfx[..p]);
            p += write_dec(&mut buf[p..], rc as u64);
            let hint: &[u8] = match rc {
                2 => b" (SERVFAIL - upstream resolver failed)",
                3 => b" (NXDOMAIN)",
                5 => b" (REFUSED)",
                _ => b"",
            };
            let hl = hint.len().min(LINE_BUF - p);
            buf[p..p + hl].copy_from_slice(&hint[..hl]);
            p += hl;
            TERM.lock().push_str(
                unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
                ERR_COL,
            );
        }
        Err(_) => {
            TERM.lock().push_str("dns: no response (timeout)", ERR_COL);
        }
    }
}

/// `http <url>` — fetch a URL via HTTP/1.0 GET and display the response body.
/// URL format: http://host[:port]/path   (https not supported)
fn cmd_http(args: &str) {
    let url = args.trim();
    if url.is_empty() {
        TERM.lock()
            .push_str("Usage: http <url>  e.g. http http://example.com/", ERR_COL);
        return;
    }

    // Strip "http://" prefix
    let rest = if url.starts_with("http://") {
        &url[7..]
    } else if url.starts_with("http:/") {
        &url[6..]
    } else {
        url
    };

    // Split host[:port] from path
    let (host_port, path) = if let Some(slash) = rest.find('/') {
        (&rest[..slash], &rest[slash..])
    } else {
        (rest, "/")
    };

    // Split host from optional :port
    let (host, port) = if let Some(colon) = host_port.rfind(':') {
        let port_str = &host_port[colon + 1..];
        let mut p = 0u16;
        let mut ok = true;
        for b in port_str.bytes() {
            if b < b'0' || b > b'9' {
                ok = false;
                break;
            }
            p = p.saturating_mul(10).saturating_add((b - b'0') as u16);
        }
        if ok && p > 0 {
            (&host_port[..colon], p)
        } else {
            (host_port, 80u16)
        }
    } else {
        (host_port, 80u16)
    };

    {
        let mut t = TERM.lock();
        let mut buf = [0u8; LINE_BUF];
        let pfx = b"GET http://";
        let mut p = pfx.len().min(LINE_BUF);
        buf[..p].copy_from_slice(&pfx[..p]);
        let hb = host.as_bytes();
        let hl = hb.len().min(LINE_BUF - p);
        buf[p..p + hl].copy_from_slice(&hb[..hl]);
        p += hl;
        let pb2 = path.as_bytes();
        let pl2 = pb2.len().min(LINE_BUF - p);
        buf[p..p + pl2].copy_from_slice(&pb2[..pl2]);
        p += pl2;
        t.push_str(
            unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
            TEXT_NORM,
        );
    }

    // Static response buffer (4 KiB — enough for most short responses)
    static HTTP_BUF: Mutex<[u8; 4096]> = Mutex::new([0u8; 4096]);
    let mut resp_buf = HTTP_BUF.lock();

    match crate::net::http::get(host, port, path, &mut resp_buf[..]) {
        Err(e) => {
            let msg = match e {
                crate::net::http::HttpError::NicNotReady => "http: NIC not ready",
                crate::net::http::HttpError::DnsTimeout => "http: DNS timeout",
                crate::net::http::HttpError::ConnectTimeout => "http: connect timeout",
                crate::net::http::HttpError::SendFailed => "http: send failed",
                crate::net::http::HttpError::ResponseTimeout => "http: response timeout",
                crate::net::http::HttpError::BufferTooSmall => {
                    "http: response truncated (buffer full)"
                }
            };
            TERM.lock().push_str(msg, ERR_COL);
        }
        Ok(n) => {
            // Find end of headers (first \r\n\r\n), display body only
            let body_start = find_body_start(&resp_buf[..n]).unwrap_or(0);
            let body = &resp_buf[body_start..n];
            // Print response line-by-line (terminal push_str takes &str)
            let mut line_start = 0usize;
            let mut lines_shown = 0usize;
            const MAX_LINES: usize = 40;
            while line_start < body.len() && lines_shown < MAX_LINES {
                let line_end = body[line_start..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map(|i| line_start + i + 1)
                    .unwrap_or(body.len());
                let line_bytes = &body[line_start..line_end];
                // Strip trailing \r\n and non-printable bytes for display
                let printable_end = line_bytes
                    .iter()
                    .rposition(|&b| b > b' ')
                    .map(|i| i + 1)
                    .unwrap_or(0);
                if printable_end > 0 {
                    // Replace non-ASCII with '?'
                    let mut display = [0u8; LINE_BUF];
                    let len = printable_end.min(LINE_BUF);
                    for (i, &b) in line_bytes[..len].iter().enumerate() {
                        display[i] = if b >= 0x20 && b < 0x80 { b } else { b'?' };
                    }
                    let s = unsafe { core::str::from_utf8_unchecked(&display[..len]) };
                    TERM.lock().push_str(s, TEXT_NORM);
                    lines_shown += 1;
                }
                line_start = line_end;
            }
            if n > 0 {
                let mut buf2 = [0u8; LINE_BUF];
                let pfx = b"--- ";
                let mut p2 = pfx.len();
                buf2[..p2].copy_from_slice(pfx);
                p2 += write_dec(&mut buf2[p2..], n as u64);
                let sfx = b" bytes received ---";
                let sl = sfx.len().min(LINE_BUF - p2);
                buf2[p2..p2 + sl].copy_from_slice(&sfx[..sl]);
                p2 += sl;
                TERM.lock().push_str(
                    unsafe { core::str::from_utf8_unchecked(&buf2[..p2]) },
                    0x88CCFF,
                );
            }
        }
    }
}

/// `netcheck [n]` — run gateway ping + DNS + HTTP checks repeatedly.
/// Default loops: 3. Max loops: 9.
fn cmd_netcheck(args: &str) {
    let loops = {
        let a = args.trim();
        if a.is_empty() {
            3usize
        } else {
            let mut v = 0usize;
            let mut ok = false;
            for b in a.bytes() {
                if b < b'0' || b > b'9' {
                    v = 0;
                    ok = false;
                    break;
                }
                ok = true;
                v = v.saturating_mul(10).saturating_add((b - b'0') as usize);
            }
            if ok {
                v.clamp(1, 9)
            } else {
                3
            }
        }
    };

    if !crate::net::driver::is_ready() {
        TERM.lock().push_str("netcheck: NIC not ready", ERR_COL);
        return;
    }
    let cfg = match crate::net::config::get() {
        Some(c) => c,
        None => {
            TERM.lock().push_str("netcheck: IP not configured", ERR_COL);
            return;
        }
    };

    let mut ping_pass = 0usize;
    let mut dns_pass = 0usize;
    let mut http_pass = 0usize;

    for i in 0..loops {
        // Header: "netcheck run 1/3"
        let mut hdr = [0u8; LINE_BUF];
        let mut hp = 0usize;
        let pfx = b"netcheck run ";
        let pl = pfx.len().min(LINE_BUF);
        hdr[..pl].copy_from_slice(&pfx[..pl]);
        hp += pl;
        hp += write_dec(&mut hdr[hp..], (i + 1) as u64);
        if hp < LINE_BUF {
            hdr[hp] = b'/';
            hp += 1;
        }
        hp += write_dec(&mut hdr[hp..], loops as u64);
        TERM.lock().push_str(
            unsafe { core::str::from_utf8_unchecked(&hdr[..hp]) },
            0x88CCFF,
        );

        // Check 1: Ping gateway
        let ping_ok = {
            let gw = cfg.gateway;
            match resolve_arp(gw) {
                Some(dst_mac) => {
                    let id = 0xB200u16;
                    let seq = i as u16;
                    crate::net::icmp::send_echo_request_to(gw, dst_mac, id, seq);
                    let deadline = crate::arch::x86_64::interrupts::uptime_ms() + 1200;
                    let mut got = false;
                    while crate::arch::x86_64::interrupts::uptime_ms() < deadline {
                        crate::net::poll_and_dispatch();
                        if let Some(reply) = crate::net::icmp::poll_reply() {
                            if reply.id == id && reply.seq == seq {
                                got = true;
                                break;
                            }
                        }
                        crate::arch::x86_64::halt::idle_once();
                    }
                    got
                }
                None => false,
            }
        };
        if ping_ok {
            ping_pass += 1;
        }
        TERM.lock().push_str(
            if ping_ok {
                "  ping: pass"
            } else {
                "  ping: fail"
            },
            if ping_ok { 0x66FF66 } else { ERR_COL },
        );

        // Check 2: DNS
        let dns_ok = crate::net::dns::resolve("example.com", 3000).is_ok();
        if dns_ok {
            dns_pass += 1;
        }
        TERM.lock().push_str(
            if dns_ok {
                "  dns:  pass"
            } else {
                "  dns:  fail"
            },
            if dns_ok { 0x66FF66 } else { ERR_COL },
        );

        // Check 3: HTTP
        static NETCHECK_HTTP_BUF: Mutex<[u8; 4096]> = Mutex::new([0u8; 4096]);
        let mut response = NETCHECK_HTTP_BUF.lock();
        let http_result = crate::net::http::get("example.com", 80, "/", &mut response[..]);
        let http_ok = match http_result {
            Ok(n) => n > 0,
            Err(crate::net::http::HttpError::BufferTooSmall) => true,
            Err(_) => false,
        };
        if http_ok {
            http_pass += 1;
        }
        let http_msg: &str = if http_ok {
            "  http: pass"
        } else {
            match http_result {
                Err(crate::net::http::HttpError::ConnectTimeout) => {
                    "  http: fail (connect timeout)"
                }
                Err(crate::net::http::HttpError::SendFailed) => "  http: fail (send failed)",
                Err(crate::net::http::HttpError::ResponseTimeout) => {
                    "  http: fail (response timeout)"
                }
                Err(crate::net::http::HttpError::DnsTimeout) => "  http: fail (dns timeout)",
                _ => "  http: fail",
            }
        };
        TERM.lock()
            .push_str(http_msg, if http_ok { 0x66FF66 } else { ERR_COL });
    }

    // Summary
    let mut l1 = [0u8; LINE_BUF];
    let mut p1 = 0usize;
    let pfx1 = b"summary ping: ";
    l1[..pfx1.len()].copy_from_slice(pfx1);
    p1 += pfx1.len();
    p1 += write_dec(&mut l1[p1..], ping_pass as u64);
    if p1 < LINE_BUF {
        l1[p1] = b'/';
        p1 += 1;
    }
    p1 += write_dec(&mut l1[p1..], loops as u64);
    TERM.lock().push_str(
        unsafe { core::str::from_utf8_unchecked(&l1[..p1]) },
        if ping_pass == loops {
            0x66FF66
        } else {
            ERR_COL
        },
    );

    let mut l2 = [0u8; LINE_BUF];
    let mut p2 = 0usize;
    let pfx2 = b"summary dns:  ";
    l2[..pfx2.len()].copy_from_slice(pfx2);
    p2 += pfx2.len();
    p2 += write_dec(&mut l2[p2..], dns_pass as u64);
    if p2 < LINE_BUF {
        l2[p2] = b'/';
        p2 += 1;
    }
    p2 += write_dec(&mut l2[p2..], loops as u64);
    TERM.lock().push_str(
        unsafe { core::str::from_utf8_unchecked(&l2[..p2]) },
        if dns_pass == loops { 0x66FF66 } else { ERR_COL },
    );

    let mut l3 = [0u8; LINE_BUF];
    let mut p3 = 0usize;
    let pfx3 = b"summary http: ";
    l3[..pfx3.len()].copy_from_slice(pfx3);
    p3 += pfx3.len();
    p3 += write_dec(&mut l3[p3..], http_pass as u64);
    if p3 < LINE_BUF {
        l3[p3] = b'/';
        p3 += 1;
    }
    p3 += write_dec(&mut l3[p3..], loops as u64);
    TERM.lock().push_str(
        unsafe { core::str::from_utf8_unchecked(&l3[..p3]) },
        if http_pass == loops {
            0x66FF66
        } else {
            ERR_COL
        },
    );
}

/// Find the offset of the HTTP body (after \r\n\r\n).
fn find_body_start(data: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i + 3 < data.len() {
        if data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n'
        {
            return Some(i + 4);
        }
        i += 1;
    }
    None
}
