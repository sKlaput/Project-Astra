use super::*;

// ---------------------------------------------------------------------------
const USER_FAULT_EXIT_CODE_VIRT: usize = 0x0000_5000_2000_0000;
const USER_FAULT_EXIT_STACK_VIRT: usize = 0x0000_5000_3000_0000;
const USER_FAULT_PF_CODE_VIRT: usize = 0x0000_5000_4000_0000;
const USER_FAULT_PF_STACK_VIRT: usize = 0x0000_5000_5000_0000;

static USER_FAULT_CANARY: AtomicU64 = AtomicU64::new(0);

fn task_user_fault_canary() {
    USER_FAULT_CANARY.fetch_add(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_user_fault_isolation() {
    // ---- Test A: SYS_WRITE_CONSOLE + SYS_EXIT from ring-3 ----
    // Bytecode (in the code page):
    //   [0..6]   mov rax, 19 (SYS_WRITE_CONSOLE)
    //   [7..16]  mov rdi, USER_FAULT_EXIT_CODE_VIRT+64  (string pointer)
    //   [17..23] mov rsi, 3  (length of "hi\n")
    //   [24..25] syscall
    //   [26..32] mov rax, 21 (SYS_EXIT)
    //   [33..35] xor rdi, rdi
    //   [36..37] syscall
    //   [38]     int3  (fallback; not reached if SYS_EXIT works)
    //   [64..66] 'h' 'i' '\n'

    let exit_code_frame = memory::frame_allocator::allocate_frame();
    let exit_stack_frame = memory::frame_allocator::allocate_frame();
    let pf_code_frame = memory::frame_allocator::allocate_frame();
    let pf_stack_frame = memory::frame_allocator::allocate_frame();

    let mut map_exit_code = false;
    let mut map_exit_stack = false;
    let mut map_pf_code = false;
    let mut map_pf_stack = false;
    let mut exit_task_ok = false;
    let mut pf_task_ok = false;
    let mut canary_ok = false;

    // Map exit-test pages
    if let Some(fr) = exit_code_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_exit_code = unsafe {
            memory::paging::map_page_current(USER_FAULT_EXIT_CODE_VIRT, fr.start_address(), flags)
                .is_ok()
        };
    }
    if let Some(fr) = exit_stack_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_exit_stack = unsafe {
            memory::paging::map_page_current(USER_FAULT_EXIT_STACK_VIRT, fr.start_address(), flags)
                .is_ok()
        };
    }

    // Map fault-test pages
    if let Some(fr) = pf_code_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_pf_code = unsafe {
            memory::paging::map_page_current(USER_FAULT_PF_CODE_VIRT, fr.start_address(), flags)
                .is_ok()
        };
    }
    if let Some(fr) = pf_stack_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_pf_stack = unsafe {
            memory::paging::map_page_current(USER_FAULT_PF_STACK_VIRT, fr.start_address(), flags)
                .is_ok()
        };
    }

    let exit_id = if map_exit_code && map_exit_stack {
        if let Some(ecf) = exit_code_frame {
            let page = unsafe {
                core::slice::from_raw_parts_mut(
                    (ecf.start_address() + memory::paging::hhdm_offset()) as *mut u8,
                    memory::paging::PAGE_SIZE,
                )
            };
            unsafe {
                core::ptr::write_bytes(page.as_mut_ptr(), 0, page.len());
            }

            // Write the test string "hi\n" at byte offset 64
            page[64] = b'h';
            page[65] = b'i';
            page[66] = b'\n';

            let str_virt = USER_FAULT_EXIT_CODE_VIRT as u64 + 64;

            // mov rax, 19 (SYS_WRITE_CONSOLE)
            page[0] = 0x48;
            page[1] = 0xC7;
            page[2] = 0xC0;
            page[3] = 19;
            page[4] = 0;
            page[5] = 0;
            page[6] = 0;
            // mov rdi, str_virt (movabs 64-bit immediate)
            page[7] = 0x48;
            page[8] = 0xBF;
            page[9..17].copy_from_slice(&str_virt.to_le_bytes());
            // mov rsi, 3
            page[17] = 0x48;
            page[18] = 0xC7;
            page[19] = 0xC6;
            page[20] = 3;
            page[21] = 0;
            page[22] = 0;
            page[23] = 0;
            // syscall
            page[24] = 0x0F;
            page[25] = 0x05;
            // mov rax, 21 (SYS_EXIT)
            page[26] = 0x48;
            page[27] = 0xC7;
            page[28] = 0xC0;
            page[29] = 21;
            page[30] = 0;
            page[31] = 0;
            page[32] = 0;
            // xor rdi, rdi
            page[33] = 0x48;
            page[34] = 0x31;
            page[35] = 0xFF;
            // syscall
            page[36] = 0x0F;
            page[37] = 0x05;
            // int3 (fallback)
            page[38] = 0xCC;

            let user_rsp = USER_FAULT_EXIT_STACK_VIRT as u64 + memory::paging::PAGE_SIZE as u64 - 8;
            scheduler::spawn_user_task_prio_name(
                USER_FAULT_EXIT_CODE_VIRT as u64,
                USER_FAULT_EXIT_STACK_VIRT as u64,
                USER_FAULT_EXIT_CODE_VIRT as u64,
                user_rsp,
                20,
                "fault-exit",
            )
        } else {
            None
        }
    } else {
        None
    };

    // Dispatch until the exit task disappears or timeout
    if let Some(exit_id) = exit_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            scheduler::dispatch_once();
            if scheduler::task_state(exit_id) == scheduler::TaskState::Empty {
                exit_task_ok = true;
                break;
            }
            idle::sleep_for_ticks(1);
        }
    }

    // ---- Test B: Ring-3 page fault isolation ----
    // Bytecode: dereference an unmapped canonical address → page fault.
    //   [0..9]  movabs rbx, 0x0000_DEAD_0000_0000
    //   [10..12] mov rax, [rbx]
    //   [13]    int3  (fallback; not reached)

    let pf_id = if map_pf_code && map_pf_stack {
        if let Some(pcf) = pf_code_frame {
            let page = unsafe {
                core::slice::from_raw_parts_mut(
                    (pcf.start_address() + memory::paging::hhdm_offset()) as *mut u8,
                    memory::paging::PAGE_SIZE,
                )
            };
            unsafe {
                core::ptr::write_bytes(page.as_mut_ptr(), 0, page.len());
            }

            const BAD_ADDR: u64 = 0x0000_DEAD_0000_0000;
            // movabs rbx, BAD_ADDR: 48 BB <8 bytes LE>
            page[0] = 0x48;
            page[1] = 0xBB;
            page[2..10].copy_from_slice(&BAD_ADDR.to_le_bytes());
            // mov rax, [rbx]: 48 8B 03
            page[10] = 0x48;
            page[11] = 0x8B;
            page[12] = 0x03;
            // int3 (fallback)
            page[13] = 0xCC;

            USER_FAULT_CANARY.store(0, Ordering::Relaxed);
            scheduler::spawn_task_with_fn_prio(task_user_fault_canary, 30);

            let user_rsp = USER_FAULT_PF_STACK_VIRT as u64 + memory::paging::PAGE_SIZE as u64 - 8;
            scheduler::spawn_user_task_prio_name(
                USER_FAULT_PF_CODE_VIRT as u64,
                USER_FAULT_PF_STACK_VIRT as u64,
                USER_FAULT_PF_CODE_VIRT as u64,
                user_rsp,
                20,
                "fault-pf",
            )
        } else {
            None
        }
    } else {
        None
    };

    if let Some(pf_id) = pf_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            scheduler::dispatch_once();
            if scheduler::task_state(pf_id) == scheduler::TaskState::Empty {
                pf_task_ok = true;
                break;
            }
            idle::sleep_for_ticks(1);
        }
        // Drain any remaining tasks (canary)
        let drain_start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(drain_start) < 40 {
            if !scheduler::dispatch_once() {
                break;
            }
        }
        canary_ok = USER_FAULT_CANARY.load(Ordering::Relaxed) > 0;
    }

    serial::write_str("arch: user-fault-isolation exit=");
    serial::write_u64(exit_task_ok as u64);
    serial::write_str(" pf=");
    serial::write_u64(pf_task_ok as u64);
    serial::write_str(" canary=");
    serial::write_u64(canary_ok as u64);
    serial::write_line("");

    let pass = exit_task_ok && pf_task_ok && canary_ok;
    serial::write_line(if pass {
        "arch: user-fault-isolation PASS"
    } else {
        "arch: user-fault-isolation FAIL"
    });
}
