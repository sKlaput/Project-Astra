use super::*;

// Keep persistent-task probe pages away from USER_FRAMEBUFFER_VIRT
// (0x0000_4000_2000_0000) to avoid virtual-range overlap.
const PERSIST_USER_CODE_VIRT: usize = 0x0000_4000_5000_0000;
const PERSIST_USER_STACK_VIRT: usize = 0x0000_4000_6000_0000;
const PERSIST_USER_SHARED_VIRT: usize = 0x0000_4000_7000_0000;
const PERSIST_USER_TARGET_COUNT: u64 = 3;
const PERSIST_USER_TRAP_RIP_OFFSET: u64 = 21;

// Persistent user-task probe: validates a scheduler-owned user task that
// repeatedly enters ring 3, increments a shared counter, traps back with int3,
// sleeps for one tick, and is then re-dispatched.
pub(crate) fn probe_persistent_user_task() {
    let code_frame = memory::frame_allocator::allocate_frame();
    let stack_frame = memory::frame_allocator::allocate_frame();
    let shared_frame = memory::frame_allocator::allocate_frame();

    let mut map_code = false;
    let mut map_stack = false;
    let mut map_shared = false;
    let mut counter = 0u64;
    let mut spawn_ok = false;
    let mut trap_hit = false;
    let mut trap_rip = 0u64;

    if let Some(frame) = code_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_code = unsafe {
            memory::paging::map_page_current(PERSIST_USER_CODE_VIRT, frame.start_address(), flags)
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
            memory::paging::map_page_current(PERSIST_USER_STACK_VIRT, frame.start_address(), flags)
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
            memory::paging::map_page_current(PERSIST_USER_SHARED_VIRT, frame.start_address(), flags)
                .is_ok()
        };
    }

    if let (Some(code_frame), Some(_stack_frame), Some(shared_frame)) =
        (code_frame, stack_frame, shared_frame)
    {
        if map_code && map_stack && map_shared {
            let code_bytes = unsafe {
                core::slice::from_raw_parts_mut(
                    (code_frame.start_address() + memory::paging::hhdm_offset()) as *mut u8,
                    memory::paging::PAGE_SIZE,
                )
            };
            let shared_ptr =
                (shared_frame.start_address() + memory::paging::hhdm_offset()) as *mut u64;

            unsafe {
                core::ptr::write_bytes(code_bytes.as_mut_ptr(), 0, code_bytes.len());
                core::ptr::write_volatile(shared_ptr, 0);
            }

            // mov rbx, PERSIST_USER_SHARED_VIRT
            code_bytes[0] = 0x48;
            code_bytes[1] = 0xBB;
            code_bytes[2..10].copy_from_slice(&(PERSIST_USER_SHARED_VIRT as u64).to_le_bytes());
            // mov rax, [rbx]
            code_bytes[10] = 0x48;
            code_bytes[11] = 0x8B;
            code_bytes[12] = 0x03;
            // add rax, 1
            code_bytes[13] = 0x48;
            code_bytes[14] = 0x83;
            code_bytes[15] = 0xC0;
            code_bytes[16] = 0x01;
            // mov [rbx], rax
            code_bytes[17] = 0x48;
            code_bytes[18] = 0x89;
            code_bytes[19] = 0x03;
            // int3
            code_bytes[20] = 0xCC;

            arch::x86_64::interrupts::arm_ring3_breakpoint_probe();

            let user_rsp = PERSIST_USER_STACK_VIRT as u64 + memory::paging::PAGE_SIZE as u64 - 8;
            let task_id = scheduler::spawn_user_task_prio_name(
                PERSIST_USER_CODE_VIRT as u64,
                PERSIST_USER_STACK_VIRT as u64,
                PERSIST_USER_CODE_VIRT as u64,
                user_rsp,
                20,
                "persist-user",
            );
            spawn_ok = task_id.is_some();

            if let Some(task_id) = task_id {
                let start = scheduler::ticks();
                while scheduler::ticks().saturating_sub(start) < 160
                    && counter < PERSIST_USER_TARGET_COUNT
                {
                    if !scheduler::dispatch_once() {
                        idle::sleep_for_ticks(1);
                    }
                    counter = unsafe { core::ptr::read_volatile(shared_ptr) };
                }

                trap_hit = arch::x86_64::interrupts::ring3_breakpoint_probe_hit();
                trap_rip = arch::x86_64::interrupts::ring3_breakpoint_probe_rip();
                scheduler::exit_task(task_id);
            }
        }
    }

    serial::write_str("arch: persistent-user-task map=");
    serial::write_u64(map_code as u64);
    serial::write_str(",");
    serial::write_u64(map_stack as u64);
    serial::write_str(",");
    serial::write_u64(map_shared as u64);
    serial::write_str(" spawn=");
    serial::write_u64(spawn_ok as u64);
    serial::write_str(" count=");
    serial::write_u64(counter);
    serial::write_str(" hit=");
    serial::write_u64(trap_hit as u64);
    serial::write_str(" rip=");
    serial::write_u64(trap_rip);
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && spawn_ok
        && counter >= PERSIST_USER_TARGET_COUNT
        && trap_hit
        && trap_rip == PERSIST_USER_CODE_VIRT as u64 + PERSIST_USER_TRAP_RIP_OFFSET;
    serial::write_line(if pass {
        "arch: persistent-user-task PASS"
    } else {
        "arch: persistent-user-task FAIL"
    });
}

// ---------------------------------------------------------------------------
// User-fault isolation probe
// Validates three E5 capabilities:
//   A. SYS_WRITE_CONSOLE (nr=19) writes text from ring-3 user code.
//   B. SYS_EXIT (nr=21) terminates a ring-3 user task cleanly.
//   C. A ring-3 page fault kills the offending task; the kernel survives.
