use super::*;

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
