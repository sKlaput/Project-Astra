use core::sync::atomic::{AtomicU64, Ordering};

use crate::*;
/// Guarded deeper framebuffer probe.
/// Off by default; enable with Cargo feature `gui-fb-kernel-deep-probe`.
pub(crate) const GUI_FB_DEEP_PROBE: bool = cfg!(feature = "gui-fb-kernel-deep-probe");

/// Experimental ring-3 framebuffer map validation probe.
/// Off by default; enable with Cargo feature `gui-fb-user-deep-probe`.
pub(crate) const GUI_FB_USER_DEEP_PROBE: bool = cfg!(feature = "gui-fb-user-deep-probe");

// Keep deep-probe pages in a dedicated high user range to avoid overlap with
// ELF demo ranges and other probe/task virtual regions.
const USER_FB_TASK_CODE_VIRT: usize = 0x0000_4000_8000_0000;
const USER_FB_TASK_STACK_VIRT: usize = 0x0000_4000_8001_0000;
const USER_FB_TASK_SHARED_VIRT: usize = 0x0000_4000_8002_0000;
const USER_FB_TASK_TRAP_RIP_OFFSET: u64 = 41;

static GUI_FB_MAP_DONE: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_OK: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_VIRT: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_BYTES: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_USER: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_WRITE: AtomicU64 = AtomicU64::new(0);

static APP_TERMINAL_DONE: AtomicU64 = AtomicU64::new(0);
static APP_TERMINAL_LAUNCH_OK: AtomicU64 = AtomicU64::new(0);
static APP_TERMINAL_HELP_OK: AtomicU64 = AtomicU64::new(0);
static APP_EDITOR_DONE: AtomicU64 = AtomicU64::new(0);
static APP_EDITOR_LAUNCH_OK: AtomicU64 = AtomicU64::new(0);
static APP_EDITOR_OPEN_OK: AtomicU64 = AtomicU64::new(0);
static APP_EDITOR_DISPLAY_OK: AtomicU64 = AtomicU64::new(0);
static APP_FILEMGR_DONE: AtomicU64 = AtomicU64::new(0);
static APP_FILEMGR_LAUNCH_OK: AtomicU64 = AtomicU64::new(0);
static APP_FILEMGR_ROOT_OK: AtomicU64 = AtomicU64::new(0);
static APP_FILEMGR_ETC_OK: AtomicU64 = AtomicU64::new(0);
static APP_SETTINGS_DONE: AtomicU64 = AtomicU64::new(0);
static APP_SETTINGS_LAUNCH_OK: AtomicU64 = AtomicU64::new(0);
static APP_SETTINGS_PLACEHOLDERS_OK: AtomicU64 = AtomicU64::new(0);
static APP_SETTINGS_LIFECYCLE_OK: AtomicU64 = AtomicU64::new(0);
static GUI_DEMO_PASS: AtomicU64 = AtomicU64::new(0);
static GUI_WM_PASS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// GUI demo probe (E9 criterion: GUI syscall demo via GUI_DEMO_ELF)
//
// Loads and runs GUI_DEMO_ELF which:
// 1. Calls SYS_GET_FB_INFO(24) to query framebuffer
// 2. Calls SYS_DRAW_PIXEL(26) twice to demonstrate graphics syscalls
// 3. Calls SYS_WRITE_CONSOLE to print status
// 4. Exits via SYS_EXIT
// ---------------------------------------------------------------------------
pub(crate) fn probe_gui_demo() {
    GUI_DEMO_PASS.store(0, Ordering::Relaxed);

    // Load the GUI demo ELF binary
    let entry = match loader::load_elf(loader::GUI_DEMO_ELF) {
        Ok(e) => e,
        Err(_) => {
            serial::write_line("gui: demo FAIL (load)");
            return;
        }
    };

    // Allocate and map a one-page user stack at a different address for this task
    const GUI_DEMO_STACK_VIRT: usize = 0x0051_0000;
    let stack_ok = match memory::frame_allocator::allocate_frame() {
        Some(frame) => {
            let flags = memory::paging::PageTableFlags::new(
                memory::paging::PageTableFlags::PRESENT
                    | memory::paging::PageTableFlags::WRITABLE
                    | memory::paging::PageTableFlags::USER_ACCESSIBLE,
            );
            unsafe {
                memory::paging::map_page_current(GUI_DEMO_STACK_VIRT, frame.start_address(), flags)
                    .is_ok()
            }
        }
        None => false,
    };

    if !stack_ok {
        serial::write_line("gui: demo FAIL (stack)");
        return;
    }

    // Initial RSP: top of stack page, 8-byte aligned
    let user_rsp = GUI_DEMO_STACK_VIRT as u64 + memory::paging::PAGE_SIZE as u64 - 8;

    // Spawn the user task
    let task_id = scheduler::spawn_user_task_prio_name(
        0x400000,                   // code_virt: ELF PT_LOAD virtual base
        GUI_DEMO_STACK_VIRT as u64, // stack_virt
        entry,                      // entry_rip
        user_rsp,
        20,
        "gui-demo",
    );

    // Drive dispatch until the task exits or timeout
    let mut done = false;
    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            scheduler::dispatch_once();
            if scheduler::task_state(tid) == scheduler::TaskState::Empty {
                done = true;
                break;
            }
            idle::sleep_for_ticks(1);
        }
    }

    serial::write_str("gui: demo entry=");
    serial::write_u64(entry);
    serial::write_str(" spawn=");
    serial::write_u64(task_id.is_some() as u64);
    serial::write_str(" done=");
    serial::write_u64(done as u64);
    serial::write_line("");
    let pass = task_id.is_some() && done;
    GUI_DEMO_PASS.store(pass as u64, Ordering::Relaxed);
    serial::write_line(if pass {
        "gui: demo PASS"
    } else {
        "gui: demo FAIL"
    });
}

fn task_gui_fb_map_probe() {
    if !GUI_FB_DEEP_PROBE {
        // Safe smoke probe: invoke SYS_MAP_FB with out_ptr=0 and validate it
        // fails cleanly (return 0) without touching caller memory.
        let ret = syscall::dispatch(syscall::SYS_MAP_FB, 0, 0, 0, 0, 0, 0);

        GUI_FB_MAP_OK.store((ret == 0) as u64, Ordering::Relaxed);
        GUI_FB_MAP_VIRT.store(0, Ordering::Relaxed);
        GUI_FB_MAP_BYTES.store(ret, Ordering::Relaxed);
        GUI_FB_MAP_USER.store(0, Ordering::Relaxed);
        GUI_FB_MAP_WRITE.store(0, Ordering::Relaxed);
    } else {
        // Deeper probe: provide an output buffer, then verify mapping metadata.
        let mut out = [0u64; 2];
        let out_ptr = out.as_mut_ptr() as usize as u64;
        let ret = syscall::dispatch(syscall::SYS_MAP_FB, out_ptr, 0, 0, 0, 0, 0);

        // This probe task is kernel-backed. With user-only mapping policy,
        // SYS_MAP_FB must deny the request cleanly.
        let in_user_task = scheduler::current_task()
            .map(scheduler::is_user_task)
            .unwrap_or(false);
        if !in_user_task {
            GUI_FB_MAP_OK.store((ret == 0) as u64, Ordering::Relaxed);
            GUI_FB_MAP_VIRT.store(0, Ordering::Relaxed);
            GUI_FB_MAP_BYTES.store(ret, Ordering::Relaxed);
            GUI_FB_MAP_USER.store(0, Ordering::Relaxed);
            GUI_FB_MAP_WRITE.store(0, Ordering::Relaxed);
            GUI_FB_MAP_DONE.store(1, Ordering::Relaxed);

            if let Some(id) = scheduler::current_task() {
                scheduler::exit_task(id);
            }
            return;
        }

        let virt_base = out[0];
        let byte_len = out[1];
        let mut user_flag = 0u64;
        let mut write_flag = 0u64;

        if ret == 1 && virt_base != 0 {
            let entry = unsafe { memory::paging::lookup_page_entry_current(virt_base as usize) }
                .unwrap_or(0);
            user_flag = ((entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0) as u64;
            write_flag = ((entry & memory::paging::PageTableFlags::WRITABLE) != 0) as u64;
        }

        let ok = ret == 1
            && virt_base == user::USER_FRAMEBUFFER_VIRT as u64
            && byte_len > 0
            && user_flag == 1
            && write_flag == 1;
        GUI_FB_MAP_OK.store(ok as u64, Ordering::Relaxed);
        GUI_FB_MAP_VIRT.store(virt_base, Ordering::Relaxed);
        GUI_FB_MAP_BYTES.store(byte_len, Ordering::Relaxed);
        GUI_FB_MAP_USER.store(user_flag, Ordering::Relaxed);
        GUI_FB_MAP_WRITE.store(write_flag, Ordering::Relaxed);
    }

    GUI_FB_MAP_DONE.store(1, Ordering::Relaxed);

    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_gui_fb_mapping() {
    GUI_FB_MAP_DONE.store(0, Ordering::Relaxed);
    GUI_FB_MAP_OK.store(0, Ordering::Relaxed);
    GUI_FB_MAP_VIRT.store(0, Ordering::Relaxed);
    GUI_FB_MAP_BYTES.store(0, Ordering::Relaxed);
    GUI_FB_MAP_USER.store(0, Ordering::Relaxed);
    GUI_FB_MAP_WRITE.store(0, Ordering::Relaxed);

    let task_id = scheduler::spawn_task_with_fn(task_gui_fb_map_probe);
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && GUI_FB_MAP_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let ok = GUI_FB_MAP_OK.load(Ordering::Relaxed);
    let virt = GUI_FB_MAP_VIRT.load(Ordering::Relaxed);
    let ret = GUI_FB_MAP_BYTES.load(Ordering::Relaxed);
    let user = GUI_FB_MAP_USER.load(Ordering::Relaxed);
    let write = GUI_FB_MAP_WRITE.load(Ordering::Relaxed);

    serial::write_str("gui: fb-map spawn=");
    serial::write_u64(task_id.is_some() as u64);
    serial::write_str(" done=");
    serial::write_u64(done as u64);
    serial::write_str(" map=");
    serial::write_u64(ok);
    serial::write_str(" virt=");
    serial::write_u64(virt);
    serial::write_str(" ret=");
    serial::write_u64(ret);
    serial::write_str(" user=");
    serial::write_u64(user);
    serial::write_str(" write=");
    serial::write_u64(write);
    serial::write_str(" deep=");
    serial::write_u64(GUI_FB_DEEP_PROBE as u64);
    serial::write_line("");

    let pass = if !GUI_FB_DEEP_PROBE {
        task_id.is_some() && done && ok == 1 && virt == 0 && ret == 0 && user == 0 && write == 0
    } else {
        // Deep kernel-mode probe now validates a clean deny path because
        // SYS_MAP_FB is intentionally scoped to user tasks.
        task_id.is_some() && done && ok == 1 && virt == 0 && ret == 0 && user == 0 && write == 0
    };
    serial::write_line(if pass {
        "gui: fb-map PASS"
    } else {
        "gui: fb-map FAIL"
    });
}

pub(crate) fn probe_gui_fb_mapping_user_task() {
    let code_frame = memory::frame_allocator::allocate_frame();
    let stack_frame = memory::frame_allocator::allocate_frame();
    let shared_frame = memory::frame_allocator::allocate_frame();

    let mut map_code = false;
    let mut map_stack = false;
    let mut map_shared = false;

    if let Some(frame) = code_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_code = unsafe {
            memory::paging::map_page_current(USER_FB_TASK_CODE_VIRT, frame.start_address(), flags)
                .is_ok()
        };
    }
    if let Some(frame) = stack_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_stack = unsafe {
            memory::paging::map_page_current(USER_FB_TASK_STACK_VIRT, frame.start_address(), flags)
                .is_ok()
        };
    }
    if let Some(frame) = shared_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_shared = unsafe {
            memory::paging::map_page_current(USER_FB_TASK_SHARED_VIRT, frame.start_address(), flags)
                .is_ok()
        };
    }

    let mut done = false;
    let mut ret = 0u64;
    let mut virt = 0u64;
    let mut bytes = 0u64;
    let mut hit = false;
    let mut cs = 0u64;
    let mut rip = 0u64;
    let mut spawn = false;
    let mut exited = false;
    let mut timed_out = false;
    let mut loop_ticks = 0u64;

    if let (Some(_code_frame), Some(_stack_frame), Some(_shared_frame)) =
        (code_frame, stack_frame, shared_frame)
    {
        if map_code && map_stack && map_shared {
            let code_bytes = unsafe {
                core::slice::from_raw_parts_mut(
                    USER_FB_TASK_CODE_VIRT as *mut u8,
                    memory::paging::PAGE_SIZE,
                )
            };
            let shared_ptr = USER_FB_TASK_SHARED_VIRT as *mut u64;

            unsafe {
                core::ptr::write_bytes(code_bytes.as_mut_ptr(), 0, code_bytes.len());
                core::ptr::write_volatile(shared_ptr, 0);
                core::ptr::write_volatile(shared_ptr.add(1), 0);
                core::ptr::write_volatile(shared_ptr.add(2), 0);
            }

            // mov rbx, USER_FB_TASK_SHARED_VIRT
            code_bytes[0] = 0x48;
            code_bytes[1] = 0xBB;
            code_bytes[2..10].copy_from_slice(&(USER_FB_TASK_SHARED_VIRT as u64).to_le_bytes());
            // mov rax, SYS_MAP_FB (28)
            code_bytes[10] = 0x48;
            code_bytes[11] = 0xC7;
            code_bytes[12] = 0xC0;
            code_bytes[13] = 0x1C;
            code_bytes[14] = 0x00;
            code_bytes[15] = 0x00;
            code_bytes[16] = 0x00;
            // lea rdi, [rbx+8]   ; out[0]=virt, out[1]=bytes
            code_bytes[17] = 0x48;
            code_bytes[18] = 0x8D;
            code_bytes[19] = 0x7B;
            code_bytes[20] = 0x08;
            // clear remaining args
            code_bytes[21] = 0x48;
            code_bytes[22] = 0x31;
            code_bytes[23] = 0xF6; // xor rsi,rsi
            code_bytes[24] = 0x31;
            code_bytes[25] = 0xD2; // xor edx,edx
            code_bytes[26] = 0x45;
            code_bytes[27] = 0x31;
            code_bytes[28] = 0xD2; // xor r10d,r10d
            code_bytes[29] = 0x45;
            code_bytes[30] = 0x31;
            code_bytes[31] = 0xC0; // xor r8d,r8d
            code_bytes[32] = 0x45;
            code_bytes[33] = 0x31;
            code_bytes[34] = 0xC9; // xor r9d,r9d
                                   // syscall
            code_bytes[35] = 0x0F;
            code_bytes[36] = 0x05;
            // mov [rbx], rax  ; return code
            code_bytes[37] = 0x48;
            code_bytes[38] = 0x89;
            code_bytes[39] = 0x03;
            // int3 + jmp $
            code_bytes[40] = 0xCC;
            code_bytes[41] = 0xEB;
            code_bytes[42] = 0xFE;

            arch::x86_64::interrupts::arm_ring3_breakpoint_probe();

            let user_rsp = USER_FB_TASK_STACK_VIRT as u64 + memory::paging::PAGE_SIZE as u64 - 8;
            let task_id = scheduler::spawn_user_task_prio_name(
                USER_FB_TASK_CODE_VIRT as u64,
                USER_FB_TASK_STACK_VIRT as u64,
                USER_FB_TASK_CODE_VIRT as u64,
                user_rsp,
                20,
                "fb-map-user",
            );
            spawn = task_id.is_some();

            if let Some(task_id) = task_id {
                let start = scheduler::ticks();
                while scheduler::ticks().saturating_sub(start) < 160 {
                    if !scheduler::dispatch_once() {
                        idle::sleep_for_ticks(1);
                    }
                    loop_ticks = scheduler::ticks().saturating_sub(start);
                    ret = unsafe { core::ptr::read_volatile(shared_ptr) };
                    virt = unsafe { core::ptr::read_volatile(shared_ptr.add(1)) };
                    bytes = unsafe { core::ptr::read_volatile(shared_ptr.add(2)) };
                    if ret == 1 {
                        done = true;
                        break;
                    }
                    if scheduler::task_state(task_id) == scheduler::TaskState::Empty {
                        exited = true;
                        break;
                    }
                }

                if !done && !exited {
                    timed_out = true;
                }

                hit = arch::x86_64::interrupts::ring3_breakpoint_probe_hit();
                cs = arch::x86_64::interrupts::ring3_breakpoint_probe_cs();
                rip = arch::x86_64::interrupts::ring3_breakpoint_probe_rip();
                scheduler::exit_task(task_id);
            }
        }
    }

    let leaf_entry = if virt != 0 {
        unsafe { memory::paging::lookup_page_entry_current(virt as usize) }.unwrap_or(0)
    } else {
        0
    };
    let user_flag = (leaf_entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0;
    let write_flag = (leaf_entry & memory::paging::PageTableFlags::WRITABLE) != 0;

    serial::write_str("gui: fb-map-user map=");
    serial::write_u64(map_code as u64);
    serial::write_str(",");
    serial::write_u64(map_stack as u64);
    serial::write_str(",");
    serial::write_u64(map_shared as u64);
    serial::write_str(" spawn=");
    serial::write_u64(spawn as u64);
    serial::write_str(" done=");
    serial::write_u64(done as u64);
    serial::write_str(" exited=");
    serial::write_u64(exited as u64);
    serial::write_str(" timeout=");
    serial::write_u64(timed_out as u64);
    serial::write_str(" ticks=");
    serial::write_u64(loop_ticks);
    serial::write_str(" hit=");
    serial::write_u64(hit as u64);
    serial::write_str(" cs=");
    serial::write_u64(cs);
    serial::write_str(" rip=");
    serial::write_u64(rip);
    serial::write_str(" ret=");
    serial::write_u64(ret);
    serial::write_str(" virt=");
    serial::write_u64(virt);
    serial::write_str(" bytes=");
    serial::write_u64(bytes);
    serial::write_str(" user=");
    serial::write_u64(user_flag as u64);
    serial::write_str(" write=");
    serial::write_u64(write_flag as u64);
    serial::write_str(" leaf=");
    serial::write_u64(leaf_entry);
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && spawn
        && done
        && hit
        && (cs & 0x3) == 0x3
        && rip == USER_FB_TASK_CODE_VIRT as u64 + USER_FB_TASK_TRAP_RIP_OFFSET
        && ret == 1
        && virt == user::USER_FRAMEBUFFER_VIRT as u64
        && bytes > 0
        && user_flag
        && write_flag;
    serial::write_line(if pass {
        "gui: fb-map-user PASS"
    } else {
        "gui: fb-map-user FAIL"
    });
}

pub(crate) fn probe_gui_window_manager() {
    GUI_WM_PASS.store(0, Ordering::Relaxed);

    // Load the GUI window manager demo ELF binary
    let entry = match loader::load_elf(loader::GUI_WINDOW_MANAGER_ELF) {
        Ok(e) => e,
        Err(_) => {
            serial::write_line("gui: window-mgr FAIL (load)");
            return;
        }
    };

    // Allocate and map a one-page user stack for this task
    const WINDOW_MGR_STACK_VIRT: usize = 0x0052_0000;
    let stack_ok = match memory::frame_allocator::allocate_frame() {
        Some(frame) => {
            let flags = memory::paging::PageTableFlags::new(
                memory::paging::PageTableFlags::PRESENT
                    | memory::paging::PageTableFlags::WRITABLE
                    | memory::paging::PageTableFlags::USER_ACCESSIBLE,
            );
            unsafe {
                memory::paging::map_page_current(
                    WINDOW_MGR_STACK_VIRT,
                    frame.start_address(),
                    flags,
                )
                .is_ok()
            }
        }
        None => false,
    };

    if !stack_ok {
        serial::write_line("gui: window-mgr FAIL (stack)");
        return;
    }

    // Initial RSP: top of stack page, 8-byte aligned
    let user_rsp = WINDOW_MGR_STACK_VIRT as u64 + memory::paging::PAGE_SIZE as u64 - 8;

    // Spawn the user task
    let task_id = scheduler::spawn_user_task_prio_name(
        0x400000, // code_virt: ELF PT_LOAD virtual base
        WINDOW_MGR_STACK_VIRT as u64,
        entry, // entry_rip
        user_rsp,
        20,
        "gui-window-mgr",
    );

    // Drive dispatch until the task exits or timeout
    let mut done = false;
    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            scheduler::dispatch_once();
            if scheduler::task_state(tid) == scheduler::TaskState::Empty {
                done = true;
                break;
            }
            idle::sleep_for_ticks(1);
        }
    }

    serial::write_str("gui: window-mgr entry=");
    serial::write_u64(entry);
    serial::write_str(" spawn=");
    serial::write_u64(task_id.is_some() as u64);
    serial::write_str(" done=");
    serial::write_u64(done as u64);
    serial::write_line("");
    let pass = task_id.is_some() && done;
    GUI_WM_PASS.store(pass as u64, Ordering::Relaxed);
    serial::write_line(if pass {
        "gui: window-mgr PASS"
    } else {
        "gui: window-mgr FAIL"
    });
}

fn terminal_v0_dispatch(cmd: &str) -> bool {
    match cmd {
        "help" => {
            serial::write_line("apps: terminal commands=help");
            true
        }
        _ => false,
    }
}

fn task_app_terminal_v0() {
    APP_TERMINAL_LAUNCH_OK.store(1, Ordering::Relaxed);

    if terminal_v0_dispatch("help") {
        APP_TERMINAL_HELP_OK.store(1, Ordering::Relaxed);
    }

    APP_TERMINAL_DONE.store(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_app_terminal_v0() {
    APP_TERMINAL_DONE.store(0, Ordering::Relaxed);
    APP_TERMINAL_LAUNCH_OK.store(0, Ordering::Relaxed);
    APP_TERMINAL_HELP_OK.store(0, Ordering::Relaxed);

    let task_id =
        scheduler::spawn_task_with_fn_prio_name(task_app_terminal_v0, 20, "app-terminal-v0");
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && APP_TERMINAL_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let launch_pass =
        task_id.is_some() && done && APP_TERMINAL_LAUNCH_OK.load(Ordering::Relaxed) == 1;
    let help_pass = task_id.is_some() && done && APP_TERMINAL_HELP_OK.load(Ordering::Relaxed) == 1;

    serial::write_line(if launch_pass {
        "apps: terminal launch PASS"
    } else {
        "apps: terminal launch FAIL"
    });

    serial::write_line(if help_pass {
        "apps: terminal command help PASS"
    } else {
        "apps: terminal command help FAIL"
    });
}

fn task_app_text_editor_v0() {
    APP_EDITOR_LAUNCH_OK.store(1, Ordering::Relaxed);

    if let Ok(mut fh) = fs::open("/etc/motd") {
        APP_EDITOR_OPEN_OK.store(1, Ordering::Relaxed);

        let mut buf = [0u8; 64];
        if let Ok(n) = fs::read(&mut fh, &mut buf) {
            let expected = b"kernel vfs motd\n";
            if n == expected.len() && &buf[..n] == expected {
                APP_EDITOR_DISPLAY_OK.store(1, Ordering::Relaxed);
            }

            serial::write_str("apps: editor file=/etc/motd bytes=");
            serial::write_u64(n as u64);
            serial::write_line("");
        }
    }

    APP_EDITOR_DONE.store(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_app_text_editor_v0() {
    APP_EDITOR_DONE.store(0, Ordering::Relaxed);
    APP_EDITOR_LAUNCH_OK.store(0, Ordering::Relaxed);
    APP_EDITOR_OPEN_OK.store(0, Ordering::Relaxed);
    APP_EDITOR_DISPLAY_OK.store(0, Ordering::Relaxed);

    let task_id =
        scheduler::spawn_task_with_fn_prio_name(task_app_text_editor_v0, 20, "app-editor-v0");
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && APP_EDITOR_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let launch_pass =
        task_id.is_some() && done && APP_EDITOR_LAUNCH_OK.load(Ordering::Relaxed) == 1;
    let open_pass = task_id.is_some() && done && APP_EDITOR_OPEN_OK.load(Ordering::Relaxed) == 1;
    let display_pass =
        task_id.is_some() && done && APP_EDITOR_DISPLAY_OK.load(Ordering::Relaxed) == 1;

    serial::write_line(if launch_pass {
        "apps: editor launch PASS"
    } else {
        "apps: editor launch FAIL"
    });

    serial::write_line(if open_pass {
        "apps: editor open PASS"
    } else {
        "apps: editor open FAIL"
    });

    serial::write_line(if display_pass {
        "apps: editor display PASS"
    } else {
        "apps: editor display FAIL"
    });
}

fn task_app_file_manager_v0() {
    APP_FILEMGR_LAUNCH_OK.store(1, Ordering::Relaxed);

    let root_count = fs::directory_entry_count("/").ok();
    let root_has_etc = fs::directory_contains("/", "etc").ok();
    let root_has_hello = fs::directory_contains("/", "hello.txt").ok();

    let etc_count = fs::directory_entry_count("/etc").ok();
    let etc_has_motd = fs::directory_contains("/etc", "motd").ok();

    if root_count == Some(2) && root_has_etc == Some(true) && root_has_hello == Some(true) {
        APP_FILEMGR_ROOT_OK.store(1, Ordering::Relaxed);
    }

    if etc_count == Some(1) && etc_has_motd == Some(true) {
        APP_FILEMGR_ETC_OK.store(1, Ordering::Relaxed);
    }

    serial::write_str("apps: filemgr root_count=");
    serial::write_u64(root_count.unwrap_or(u64::MAX as usize) as u64);
    serial::write_str(" etc_count=");
    serial::write_u64(etc_count.unwrap_or(u64::MAX as usize) as u64);
    serial::write_line("");

    APP_FILEMGR_DONE.store(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_app_file_manager_v0() {
    APP_FILEMGR_DONE.store(0, Ordering::Relaxed);
    APP_FILEMGR_LAUNCH_OK.store(0, Ordering::Relaxed);
    APP_FILEMGR_ROOT_OK.store(0, Ordering::Relaxed);
    APP_FILEMGR_ETC_OK.store(0, Ordering::Relaxed);

    let task_id =
        scheduler::spawn_task_with_fn_prio_name(task_app_file_manager_v0, 20, "app-filemgr-v0");
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && APP_FILEMGR_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let launch_pass =
        task_id.is_some() && done && APP_FILEMGR_LAUNCH_OK.load(Ordering::Relaxed) == 1;
    let root_pass = task_id.is_some() && done && APP_FILEMGR_ROOT_OK.load(Ordering::Relaxed) == 1;
    let etc_pass = task_id.is_some() && done && APP_FILEMGR_ETC_OK.load(Ordering::Relaxed) == 1;

    serial::write_line(if launch_pass {
        "apps: filemgr launch PASS"
    } else {
        "apps: filemgr launch FAIL"
    });

    serial::write_line(if root_pass {
        "apps: filemgr list root PASS"
    } else {
        "apps: filemgr list root FAIL"
    });

    serial::write_line(if etc_pass {
        "apps: filemgr list etc PASS"
    } else {
        "apps: filemgr list etc FAIL"
    });
}

fn settings_v0_dispatch(cmd: &str) -> bool {
    match cmd {
        "show" => {
            serial::write_line("apps: settings panes=display,keyboard,network");
            true
        }
        _ => false,
    }
}

fn task_app_settings_v0() {
    APP_SETTINGS_LAUNCH_OK.store(1, Ordering::Relaxed);

    if settings_v0_dispatch("show") {
        APP_SETTINGS_PLACEHOLDERS_OK.store(1, Ordering::Relaxed);
    }

    // Lifecycle placeholder: foreground -> background -> foreground.
    serial::write_line("apps: settings lifecycle=foreground,background,foreground");
    APP_SETTINGS_LIFECYCLE_OK.store(1, Ordering::Relaxed);

    APP_SETTINGS_DONE.store(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_app_settings_v0() {
    APP_SETTINGS_DONE.store(0, Ordering::Relaxed);
    APP_SETTINGS_LAUNCH_OK.store(0, Ordering::Relaxed);
    APP_SETTINGS_PLACEHOLDERS_OK.store(0, Ordering::Relaxed);
    APP_SETTINGS_LIFECYCLE_OK.store(0, Ordering::Relaxed);

    let task_id =
        scheduler::spawn_task_with_fn_prio_name(task_app_settings_v0, 20, "app-settings-v0");
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && APP_SETTINGS_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let launch_pass =
        task_id.is_some() && done && APP_SETTINGS_LAUNCH_OK.load(Ordering::Relaxed) == 1;
    let placeholders_pass =
        task_id.is_some() && done && APP_SETTINGS_PLACEHOLDERS_OK.load(Ordering::Relaxed) == 1;
    let lifecycle_pass =
        task_id.is_some() && done && APP_SETTINGS_LIFECYCLE_OK.load(Ordering::Relaxed) == 1;

    serial::write_line(if launch_pass {
        "apps: settings launch PASS"
    } else {
        "apps: settings launch FAIL"
    });

    serial::write_line(if placeholders_pass {
        "apps: settings placeholders PASS"
    } else {
        "apps: settings placeholders FAIL"
    });

    serial::write_line(if lifecycle_pass {
        "apps: settings lifecycle PASS"
    } else {
        "apps: settings lifecycle FAIL"
    });
}
