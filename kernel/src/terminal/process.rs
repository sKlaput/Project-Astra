fn cmd_exec(args: &str) {
    let prog = args.trim();
    let (elf, prog_name): (&[u8], &'static str) = match prog {
        "hello" => (crate::loader::HELLO_ELF, "hello"),
        "gui" => (crate::loader::GUI_DEMO_ELF, "gui"),
        "nxbomb" => (crate::loader::NXBOMB_ELF, "nxbomb"),
        "stackbomb" => (crate::loader::STACKBOMB_ELF, "stackbomb"),
        _ => {
            let mut t = TERM.lock();
            t.push_str("exec: unknown program", ERR_COL);
            t.push_str("  known: hello  gui  nxbomb  stackbomb", TEXT_NORM);
            return;
        }
    };

    // Reject if a user process is already running (shared page tables, fixed vaddrs).
    if crate::process::count_running_user() > 0 {
        TERM.lock()
            .push_str("exec: a user process is already running", ERR_COL);
        return;
    }

    match crate::process::spawn_elf_process(prog_name, elf, crate::user::USER_TASK_STACK_VIRT, 128)
    {
        Some(pid) => {
            let mut t = TERM.lock();
            let mut buf = [0u8; LINE_BUF];
            let mut p = 0usize;
            let pfx = b"spawned  pid=";
            buf[..pfx.len()].copy_from_slice(pfx);
            p += pfx.len();
            p += write_dec(&mut buf[p..], pid.0);
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
            t.push_str(s, 0x66FF66);
        }
        None => {
            TERM.lock().push_str("exec: spawn failed", ERR_COL);
        }
    }
}

fn cmd_ps() {
    let (entries, count) = crate::process::list_all();
    let mut t = TERM.lock();
    if count == 0 {
        t.push_str("no processes", TEXT_NORM);
        return;
    }
    t.push_str("PID  STATE    TASK  NAME", TEXT_NORM);
    for i in 0..count {
        let e = &entries[i];
        let state_str: &[u8] = match e.state {
            crate::process::ProcessState::Running => b"running ",
            crate::process::ProcessState::Exited => b"exited  ",
            crate::process::ProcessState::Empty => b"empty   ",
        };
        // Build "PID  STATE    TASK  name"
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        p += write_dec(&mut buf[p..], e.pid);
        buf[p] = b' ';
        p += 1;
        let sl = state_str.len().min(LINE_BUF - p);
        buf[p..p + sl].copy_from_slice(&state_str[..sl]);
        p += sl;
        p += write_dec(&mut buf[p..], e.task_id);
        buf[p] = b' ';
        p += 1;
        let nl = e.name_len.min(LINE_BUF - p);
        buf[p..p + nl].copy_from_slice(&e.name[..nl]);
        p += nl;
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        let col = if e.state == crate::process::ProcessState::Running {
            0x66FF66
        } else {
            0xAAAAAA
        };
        t.push_str(s, col);
    }
}

fn cmd_kill(args: &str) {
    let pid_str = args.trim();
    let mut pid_val = 0u64;
    for b in pid_str.bytes() {
        if b < b'0' || b > b'9' {
            break;
        }
        pid_val = pid_val * 10 + (b - b'0') as u64;
    }
    if pid_val == 0 {
        TERM.lock().push_str("kill: usage: kill <pid>", ERR_COL);
        return;
    }
    let pid = crate::process::ProcessId(pid_val);
    match crate::process::main_task(pid) {
        Some(task_id) => {
            crate::scheduler::exit_task(task_id);
            let mut t = TERM.lock();
            let mut buf = [0u8; LINE_BUF];
            let mut p = 0usize;
            let pfx = b"killed pid=";
            buf[..pfx.len()].copy_from_slice(pfx);
            p += pfx.len();
            p += write_dec(&mut buf[p..], pid_val);
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
            t.push_str(s, TEXT_NORM);
        }
        None => {
            TERM.lock().push_str("kill: no such process", ERR_COL);
        }
    }
}
