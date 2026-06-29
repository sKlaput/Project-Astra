use super::*;

pub(crate) fn probe_ring3_breakpoint_roundtrip() {
    const USER_CODE_VIRT: usize = 0x0000_0000_0040_3000;
    const USER_STACK_VIRT: usize = 0x0000_0000_0040_4000;
    const USER_SHARED_VIRT: usize = 0x0000_0000_0040_5000;
    const USER_STACK_TOP: usize = USER_STACK_VIRT + memory::paging::PAGE_SIZE - 16;
    const USER_MARKER: u64 = 0x5249_4E47_335F_4F4B;
    const USER_TRAP_RIP_OFFSET: u64 = 24;

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

    let shared_value;
    let trap_hit;
    let trap_cs;
    let trap_rip;

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
            core::ptr::write_volatile(shared_ptr, 0);
        }

        code_bytes[0] = 0x48;
        code_bytes[1] = 0xB8;
        code_bytes[2..10].copy_from_slice(&(USER_SHARED_VIRT as u64).to_le_bytes());
        code_bytes[10] = 0x48;
        code_bytes[11] = 0xBB;
        code_bytes[12..20].copy_from_slice(&USER_MARKER.to_le_bytes());
        code_bytes[20] = 0x48;
        code_bytes[21] = 0x89;
        code_bytes[22] = 0x18;
        code_bytes[23] = 0xCC;
        code_bytes[24] = 0xEB;
        code_bytes[25] = 0xFE;

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

        shared_value = unsafe { core::ptr::read_volatile(shared_ptr) };
        trap_hit = arch::x86_64::interrupts::ring3_breakpoint_probe_hit();
        trap_cs = arch::x86_64::interrupts::ring3_breakpoint_probe_cs();
        trap_rip = arch::x86_64::interrupts::ring3_breakpoint_probe_rip();
        arch::x86_64::ring3::clear_saved_resume_rsp();
    } else {
        shared_value = 0;
        trap_hit = false;
        trap_cs = 0;
        trap_rip = 0;
    }

    serial::write_str("arch: ring3-run map=");
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
    serial::write_str(" shared=");
    serial::write_u64(shared_value);
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && trap_hit
        && (trap_cs & 0x3) == 0x3
        && trap_rip == USER_CODE_VIRT as u64 + USER_TRAP_RIP_OFFSET
        && shared_value == USER_MARKER;
    serial::write_line(if pass {
        "arch: ring3-run PASS"
    } else {
        "arch: ring3-run FAIL"
    });
}
