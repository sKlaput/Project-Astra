// ── Internal: command execution ───────────────────────────────────────────────

fn execute_input() {
    let (cmd_data, cmd_len) = {
        let t = TERM.lock();
        let mut d = [0u8; LINE_BUF];
        let l = t.input_len;
        d[..l].copy_from_slice(&t.input[..l]);
        (d, l)
    };

    // Echo prompt + command to history, push to cmd history, clear input
    {
        let mut t = TERM.lock();
        let mut echo = [0u8; LINE_BUF];
        let pb = PROMPT.as_bytes();
        let pn = pb.len().min(LINE_BUF);
        echo[..pn].copy_from_slice(&pb[..pn]);
        let cn = cmd_len.min(LINE_BUF - pn);
        echo[pn..pn + cn].copy_from_slice(&cmd_data[..cn]);
        t.push_bytes(&echo[..pn + cn], PROMPT_COL);
        t.push_cmd_hist(&cmd_data, cmd_len);
        t.input_len = 0;
        t.cursor_pos = 0;
        t.cmd_hpos = 0;
    }

    if cmd_len == 0 {
        return;
    }

    let raw = unsafe { core::str::from_utf8_unchecked(&cmd_data[..cmd_len]) };
    let raw = raw.trim_end();
    if raw.is_empty() {
        return;
    }

    let (cmd, args) = match raw.find(' ') {
        Some(pos) => (&raw[..pos], raw[pos + 1..].trim_start()),
        None => (raw, ""),
    };

    run_cmd(cmd, args);
}

fn run_cmd(cmd: &str, args: &str) {
    match cmd {
        "help" => {
            let mut t = TERM.lock();
            t.push_str("Commands:", TEXT_NORM);
            t.push_str("  help              - this list", TEXT_NORM);
            t.push_str("  clear             - clear screen", TEXT_NORM);
            t.push_str("  version           - OS version", TEXT_NORM);
            t.push_str("  uptime            - time since boot", TEXT_NORM);
            t.push_str("  mem               - heap memory usage", TEXT_NORM);
            t.push_str("  ls [path]         - list directory", TEXT_NORM);
            t.push_str("  cd <dir>          - change directory", TEXT_NORM);
            t.push_str("  cat <file>        - print file contents", TEXT_NORM);
            t.push_str("  touch <name>      - create empty file", TEXT_NORM);
            t.push_str("  mkdir <name>      - create directory", TEXT_NORM);
            t.push_str("  rm <name>         - delete file or folder", TEXT_NORM);
            t.push_str("  rename <old> <new>- rename entry", TEXT_NORM);
            t.push_str("  cp <src> <dst>    - copy file", TEXT_NORM);
            t.push_str("  mv <src> <dst>    - move/rename file", TEXT_NORM);
            t.push_str("  net               - network status", TEXT_NORM);
            t.push_str("  ping <ip>         - send ICMP echo to <ip>", TEXT_NORM);
            t.push_str("  dns <host>        - resolve hostname via DNS", TEXT_NORM);
            t.push_str(
                "  http <url>        - HTTP GET (e.g. http http://example.com/)",
                TEXT_NORM,
            );
            t.push_str(
                "  netcheck [n]      - run ping/dns/http checks n times (default 3)",
                TEXT_NORM,
            );
            t.push_str(
                "  exec <prog>        - run user program (hello/gui)",
                TEXT_NORM,
            );
            t.push_str("  ps                 - list processes", TEXT_NORM);
            t.push_str("  kill <pid>         - terminate process", TEXT_NORM);
            t.push_str(
                "  memprobe          - kernel/user isolation diagnostic",
                TEXT_NORM,
            );
            t.push_str(
                "  memtest           - pointer-validation regression battery",
                TEXT_NORM,
            );
            t.push_str(
                "  cpuinfo           - CPU vendor/brand, APIC, topology",
                TEXT_NORM,
            );
            t.push_str(
                "  apictest          - switch tick source PIT->LAPIC->PIT",
                TEXT_NORM,
            );
            t.push_str("  echo <text>        - print text", TEXT_NORM);
            t.push_str("  Up/Down arrows    - command history", TEXT_NORM);
        }

        "clear" => {
            TERM.lock().hist_cnt = 0;
        }

        "version" => {
            let mut t = TERM.lock();
            t.push_str("Astra OS  v0.1", TEXT_NORM);
            t.push_str("Kernel: Rust no_std / UEFI / x86_64", TEXT_NORM);
            t.push_str("Build:  April 2026", TEXT_NORM);
        }

        "uptime" => {
            let ms = crate::arch::x86_64::interrupts::uptime_ms();
            let secs = ms / 1000;
            let millis = ms % 1000;
            let mut buf = [0u8; 48];
            let mut pos = 0;
            let pfx = b"Uptime: ";
            buf[..pfx.len()].copy_from_slice(pfx);
            pos += pfx.len();
            pos += write_dec(&mut buf[pos..], secs);
            buf[pos] = b's';
            pos += 1;
            buf[pos] = b'.';
            pos += 1;
            pos += write_dec(&mut buf[pos..], millis);
            buf[pos] = b'm';
            pos += 1;
            buf[pos] = b's';
            pos += 1;
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..pos]) };
            TERM.lock().push_str(s, TEXT_NORM);
        }

        "mem" => {
            let heap = crate::memory::heap::get_telemetry();
            let used_kb = heap.used_bytes / 1024;
            let total_kb = (heap.mapped_pages * 4096) / 1024;
            let free_kb = total_kb.saturating_sub(used_kb);
            let mut t = TERM.lock();
            // "Used:  1234 KB / 8192 KB  (15%)"
            let mut buf = [0u8; LINE_BUF];
            let mut pos = 0;
            let pfx = b"Heap used:  ";
            buf[..pfx.len()].copy_from_slice(pfx);
            pos += pfx.len();
            pos += write_dec(&mut buf[pos..], used_kb as u64);
            let mid = b" KB / ";
            buf[pos..pos + mid.len()].copy_from_slice(mid);
            pos += mid.len();
            pos += write_dec(&mut buf[pos..], total_kb as u64);
            let sfx = b" KB";
            buf[pos..pos + sfx.len()].copy_from_slice(sfx);
            pos += sfx.len();
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..pos]) };
            t.push_str(s, TEXT_NORM);

            let mut buf2 = [0u8; LINE_BUF];
            let mut p2 = 0;
            let pfx2 = b"Heap free:  ";
            buf2[..pfx2.len()].copy_from_slice(pfx2);
            p2 += pfx2.len();
            p2 += write_dec(&mut buf2[p2..], free_kb as u64);
            let sfx2 = b" KB";
            buf2[p2..p2 + sfx2.len()].copy_from_slice(sfx2);
            p2 += sfx2.len();
            let s2 = unsafe { core::str::from_utf8_unchecked(&buf2[..p2]) };
            t.push_str(s2, TEXT_NORM);
        }

        "ls" => {
            cmd_ls(args);
        }

        "cd" => {
            cmd_cd(args);
        }

        "cat" => {
            cmd_cat(args);
        }

        "touch" => {
            cmd_touch(args);
        }

        "mkdir" => {
            cmd_mkdir(args);
        }

        "rm" => {
            cmd_rm(args);
        }

        "rename" => {
            cmd_rename(args);
        }

        "cp" => {
            cmd_cp(args);
        }

        "mv" => {
            cmd_mv(args);
        }

        "net" => {
            cmd_net();
        }

        "ping" => {
            cmd_ping(args);
        }

        "dns" => {
            cmd_dns(args);
        }

        "http" => {
            cmd_http(args);
        }

        "netcheck" => {
            cmd_netcheck(args);
        }

        "exec" => {
            cmd_exec(args);
        }

        "ps" => {
            cmd_ps();
        }

        "kill" => {
            cmd_kill(args);
        }

        "memprobe" => {
            cmd_memprobe();
        }

        "memtest" => {
            cmd_memtest();
        }

        "cpuinfo" => {
            cmd_cpuinfo();
        }

        "apictest" => {
            cmd_apictest();
        }

        "echo" => {
            let text = if args.is_empty() { "" } else { args };
            TERM.lock().push_str(text, TEXT_NORM);
        }

        other => {
            let mut t = TERM.lock();
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"Unknown command: ";
            let pn = pfx.len().min(LINE_BUF);
            buf[..pn].copy_from_slice(&pfx[..pn]);
            let ob = other.as_bytes();
            let on = ob.len().min(LINE_BUF - pn);
            buf[pn..pn + on].copy_from_slice(&ob[..on]);
            t.push_bytes(&buf[..pn + on], ERR_COL);
        }
    }
}
