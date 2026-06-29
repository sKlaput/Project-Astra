use super::*;

pub(crate) fn probe_syscall_abi_smoke_user() {
    const USER_CODE_VIRT: usize = 0x0000_0000_0041_6000;
    const USER_STACK_VIRT: usize = 0x0000_0000_0041_7000;
    const USER_SHARED_VIRT: usize = 0x0000_0000_0041_8000;
    const USER_STACK_TOP: usize = USER_STACK_VIRT + memory::paging::PAGE_SIZE - 16;

    const OFF_TASK_ID: usize = 0;
    const OFF_TICKS: usize = 8;
    const OFF_ADD: usize = 16;
    const OFF_MAX: usize = 24;
    const OFF_ENOSYS: usize = 32;
    const OFF_PENDING: usize = 40;

    let code_frame = memory::frame_allocator::allocate_frame();
    let stack_frame = memory::frame_allocator::allocate_frame();
    let shared_frame = memory::frame_allocator::allocate_frame();

    let mut map_code = false;
    let mut map_stack = false;
    let mut map_shared = false;

    if let Some(frame) = code_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_code = unsafe {
            memory::paging::map_page_current(USER_CODE_VIRT, frame.start_address(), flags).is_ok()
        };
    }
    if let Some(frame) = stack_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_stack = unsafe {
            memory::paging::map_page_current(USER_STACK_VIRT, frame.start_address(), flags).is_ok()
        };
    }
    if let Some(frame) = shared_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_shared = unsafe {
            memory::paging::map_page_current(USER_SHARED_VIRT, frame.start_address(), flags).is_ok()
        };
    }

    let task_id;
    let ticks;
    let add_ret;
    let max_ret;
    let enosys_ret;
    let pending_ret;
    let trap_hit;
    let trap_cs;
    let trap_rip;
    let mut trap_expected_rip = 0u64;

    if let (Some(code_frame), Some(_stack_frame), Some(shared_frame)) =
        (code_frame, stack_frame, shared_frame)
    {
        let code_phys = code_frame.start_address();
        let shared_phys = shared_frame.start_address();
        let code_bytes = unsafe {
            core::slice::from_raw_parts_mut(
                (code_phys + memory::paging::hhdm_offset()) as *mut u8,
                memory::paging::PAGE_SIZE,
            )
        };
        let shared_ptr = (shared_phys + memory::paging::hhdm_offset()) as *mut u64;

        unsafe {
            core::ptr::write_bytes(code_bytes.as_mut_ptr(), 0, code_bytes.len());
            core::ptr::write_bytes(shared_ptr as *mut u8, 0, 48);
        }

        let mut cursor = 0usize;
        macro_rules! emit_bytes {
            ($src:expr) => {{
                let src = $src;
                let end = cursor + src.len();
                code_bytes[cursor..end].copy_from_slice(src);
                cursor = end;
            }};
        }
        macro_rules! emit_u64 {
            ($value:expr) => {{
                let b = ($value as u64).to_le_bytes();
                let end = cursor + 8;
                code_bytes[cursor..end].copy_from_slice(&b);
                cursor = end;
            }};
        }

        // r13 = shared base
        emit_bytes!(&[0x49, 0xBD]);
        emit_u64!(USER_SHARED_VIRT as u64);

        // SYS_TASK_ID -> [shared+0], and keep in r12
        emit_bytes!(&[0x48, 0xC7, 0xC0, 0x05, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x31, 0xFF, 0x31, 0xF6, 0x31, 0xD2]);
        emit_bytes!(&[0x45, 0x31, 0xD2, 0x45, 0x31, 0xC0, 0x45, 0x31, 0xC9]);
        emit_bytes!(&[0x0F, 0x05]);
        emit_bytes!(&[0x49, 0x89, 0xC4]);
        emit_bytes!(&[0x49, 0x89, 0x45, OFF_TASK_ID as u8]);

        // SYS_TICKS -> [shared+8]
        emit_bytes!(&[0x48, 0xC7, 0xC0, 0x04, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x31, 0xFF, 0x31, 0xF6, 0x31, 0xD2]);
        emit_bytes!(&[0x45, 0x31, 0xD2, 0x45, 0x31, 0xC0, 0x45, 0x31, 0xC9]);
        emit_bytes!(&[0x0F, 0x05]);
        emit_bytes!(&[0x49, 0x89, 0x45, OFF_TICKS as u8]);

        // SYS_ADD(17,34) -> 51 -> [shared+16]
        emit_bytes!(&[0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x48, 0xC7, 0xC7, 0x11, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x48, 0xC7, 0xC6, 0x22, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x31, 0xD2, 0x45, 0x31, 0xD2, 0x45, 0x31, 0xC0, 0x45, 0x31, 0xC9]);
        emit_bytes!(&[0x0F, 0x05]);
        emit_bytes!(&[0x49, 0x89, 0x45, OFF_ADD as u8]);

        // SYS_MAX(3,9) -> 9 -> [shared+24]
        emit_bytes!(&[0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x48, 0xC7, 0xC7, 0x03, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x48, 0xC7, 0xC6, 0x09, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x31, 0xD2, 0x45, 0x31, 0xD2, 0x45, 0x31, 0xC0, 0x45, 0x31, 0xC9]);
        emit_bytes!(&[0x0F, 0x05]);
        emit_bytes!(&[0x49, 0x89, 0x45, OFF_MAX as u8]);

        // invalid nr=255 -> ENOSYS -> [shared+32]
        emit_bytes!(&[0x48, 0xC7, 0xC0, 0xFF, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x31, 0xFF, 0x31, 0xF6, 0x31, 0xD2]);
        emit_bytes!(&[0x45, 0x31, 0xD2, 0x45, 0x31, 0xC0, 0x45, 0x31, 0xC9]);
        emit_bytes!(&[0x0F, 0x05]);
        emit_bytes!(&[0x49, 0x89, 0x45, OFF_ENOSYS as u8]);

        // SYS_SIGNAL_PENDING(task_id) -> usually 0 -> [shared+40]
        emit_bytes!(&[0x48, 0xC7, 0xC0, 0x07, 0x00, 0x00, 0x00]);
        emit_bytes!(&[0x4C, 0x89, 0xE7]);
        emit_bytes!(&[0x31, 0xF6, 0x31, 0xD2]);
        emit_bytes!(&[0x45, 0x31, 0xD2, 0x45, 0x31, 0xC0, 0x45, 0x31, 0xC9]);
        emit_bytes!(&[0x0F, 0x05]);
        emit_bytes!(&[0x49, 0x89, 0x45, OFF_PENDING as u8]);

        let success_int3 = cursor;
        emit_bytes!(&[0xCC]);
        emit_bytes!(&[0xEB, 0xFE]);
        let _ = cursor;
        trap_expected_rip = USER_CODE_VIRT as u64 + success_int3 as u64 + 1;

        arch::x86_64::ring3::clear_saved_resume_rsp();
        arch::x86_64::interrupts::arm_ring3_breakpoint_probe();

        let mut raw_rflags: u64;
        unsafe {
            core::arch::asm!("pushfq", "pop {}", out(reg) raw_rflags, options(nomem, preserves_flags));
        }
        let user_rflags = (RFlags::from_bits_truncate(raw_rflags) | RFlags::INTERRUPT_FLAG).bits();

        unsafe {
            arch::x86_64::ring3::enter_user_mode(
                USER_CODE_VIRT as u64,
                USER_STACK_TOP as u64,
                arch::x86_64::ring3_code_selector().0 as u64,
                arch::x86_64::ring3_data_selector().0 as u64,
                user_rflags,
            );
        }

        task_id = unsafe { core::ptr::read_volatile(shared_ptr.add(OFF_TASK_ID / 8)) };
        ticks = unsafe { core::ptr::read_volatile(shared_ptr.add(OFF_TICKS / 8)) };
        add_ret = unsafe { core::ptr::read_volatile(shared_ptr.add(OFF_ADD / 8)) };
        max_ret = unsafe { core::ptr::read_volatile(shared_ptr.add(OFF_MAX / 8)) };
        enosys_ret = unsafe { core::ptr::read_volatile(shared_ptr.add(OFF_ENOSYS / 8)) };
        pending_ret = unsafe { core::ptr::read_volatile(shared_ptr.add(OFF_PENDING / 8)) };
        trap_hit = arch::x86_64::interrupts::ring3_breakpoint_probe_hit();
        trap_cs = arch::x86_64::interrupts::ring3_breakpoint_probe_cs();
        trap_rip = arch::x86_64::interrupts::ring3_breakpoint_probe_rip();
        arch::x86_64::ring3::clear_saved_resume_rsp();
    } else {
        task_id = 0;
        ticks = 0;
        add_ret = 0;
        max_ret = 0;
        enosys_ret = 0;
        pending_ret = 0;
        trap_hit = false;
        trap_cs = 0;
        trap_rip = 0;
    }

    serial::write_str("arch: syscall-abi map=");
    serial::write_u64(map_code as u64);
    serial::write_str(",");
    serial::write_u64(map_stack as u64);
    serial::write_str(",");
    serial::write_u64(map_shared as u64);
    serial::write_str(" hit=");
    serial::write_u64(trap_hit as u64);
    serial::write_str(" cs=");
    serial::write_u64(trap_cs);
    serial::write_str(" rip=");
    serial::write_u64(trap_rip);
    serial::write_str(" tid=");
    serial::write_u64(task_id);
    serial::write_str(" ticks=");
    serial::write_u64(ticks);
    serial::write_str(" add=");
    serial::write_u64(add_ret);
    serial::write_str(" max=");
    serial::write_u64(max_ret);
    serial::write_str(" enosys=");
    serial::write_u64(enosys_ret);
    serial::write_str(" pend=");
    serial::write_u64(pending_ret);
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && trap_hit
        && (trap_cs & 0x3) == 0x3
        && trap_rip == trap_expected_rip
        // Current ring-3 probes execute outside scheduler task context, so
        // SYS_TASK_ID returns 0 until user tasks are integrated.
        && task_id == 0
        && ticks != 0
        && add_ret == 51
        && max_ret == 9
        && enosys_ret == syscall::SYS_ENOSYS
        && pending_ret == 0;
    serial::write_line(if pass {
        "arch: syscall-abi PASS"
    } else {
        "arch: syscall-abi FAIL"
    });
}
