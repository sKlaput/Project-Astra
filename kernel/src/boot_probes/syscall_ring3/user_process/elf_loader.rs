use super::*;

// ---------------------------------------------------------------------------
// ELF loader probe (E5 criterion 2: statically linked ELF user program loads
// and runs).
//
// Loads the embedded HELLO_ELF binary via loader::load_elf(), maps a one-page
// user stack at ELF_USER_STACK_VIRT, spawns the task through the normal
// scheduler user-task path, and drives dispatch until the task self-exits via
// SYS_EXIT.  Expected serial output from user space: "hello from elf\n".
// ---------------------------------------------------------------------------
const ELF_USER_STACK_VIRT: usize = 0x0050_0000;

pub(crate) fn probe_elf_loader() {
    // Load the ELF binary (maps its PT_LOAD segment at 0x400000 with R+X).
    let entry = match loader::load_elf(loader::HELLO_ELF) {
        Ok(e) => e,
        Err(_) => {
            serial::write_line("arch: elf-loader FAIL (load)");
            return;
        }
    };

    // Allocate and map a one-page user stack.
    let stack_ok = match memory::frame_allocator::allocate_frame() {
        Some(frame) => {
            let flags = memory::paging::PageTableFlags::new(
                memory::paging::PageTableFlags::PRESENT
                    | memory::paging::PageTableFlags::WRITABLE
                    | memory::paging::PageTableFlags::USER_ACCESSIBLE,
            );
            unsafe {
                memory::paging::map_page_current(ELF_USER_STACK_VIRT, frame.start_address(), flags)
                    .is_ok()
            }
        }
        None => false,
    };

    if !stack_ok {
        serial::write_line("arch: elf-loader FAIL (stack)");
        return;
    }

    // Initial RSP: top of stack page, 8-byte aligned.
    let user_rsp = ELF_USER_STACK_VIRT as u64 + memory::paging::PAGE_SIZE as u64 - 8;

    // Spawn the user task via the standard scheduler trampoline.
    let task_id = scheduler::spawn_user_task_prio_name(
        0x400000,                   // code_virt: ELF PT_LOAD virtual base
        ELF_USER_STACK_VIRT as u64, // stack_virt
        entry,                      // entry_rip: from ELF e_entry
        user_rsp,
        20,
        "elf-hello",
    );

    // Drive dispatch until the task exits (via SYS_EXIT) or timeout.
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

    serial::write_str("arch: elf-loader entry=");
    serial::write_u64(entry);
    serial::write_str(" spawn=");
    serial::write_u64(task_id.is_some() as u64);
    serial::write_str(" done=");
    serial::write_u64(done as u64);
    serial::write_line("");
    serial::write_line(if task_id.is_some() && done {
        "arch: elf-loader PASS"
    } else {
        "arch: elf-loader FAIL"
    });
}
