use super::*;

pub(crate) fn probe_syscall_sysret_stack_stress() {
    const USER_CODE_VIRT: usize = 0x0000_0000_0041_3000;
    const USER_STACK_VIRT: usize = 0x0000_0000_0041_4000;
    const USER_SHARED_VIRT: usize = 0x0000_0000_0041_5000;
    const USER_STACK_TOP: usize = USER_STACK_VIRT + memory::paging::PAGE_SIZE - 16;
    const LOOP_COUNT: u64 = 32;
    const FAIL_MARKER: u64 = 0xBAD0_BAD0_BAD0_BAD0;

    let expected_result = LOOP_COUNT.wrapping_mul(LOOP_COUNT.wrapping_add(3)) / 2;

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
            core::ptr::write_volatile(shared_ptr, 0);
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

        // r12 = shared result address, r14 = loop counter, r15 = accumulator.
        emit_bytes!(&[0x49, 0xBC]);
        emit_u64!(USER_SHARED_VIRT as u64);
        emit_bytes!(&[0x49, 0xBE]);
        emit_u64!(LOOP_COUNT);
        emit_bytes!(&[0x4D, 0x31, 0xFF]);

        let loop_start = cursor;

        // Stack stress per iteration: push payload to user stack and verify readback.
        emit_bytes!(&[0x48, 0x83, 0xEC, 0x08]); // sub rsp, 8
        emit_bytes!(&[0x4C, 0x89, 0x34, 0x24]); // mov [rsp], r14

        // syscall SYS_ADD(r14, 1)
        emit_bytes!(&[0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00]); // mov rax, 1
        emit_bytes!(&[0x4C, 0x89, 0xF7]); // mov rdi, r14
        emit_bytes!(&[0x48, 0xC7, 0xC6, 0x01, 0x00, 0x00, 0x00]); // mov rsi, 1
        emit_bytes!(&[0x31, 0xD2]); // xor edx, edx
        emit_bytes!(&[0x45, 0x31, 0xD2]); // xor r10d, r10d
        emit_bytes!(&[0x45, 0x31, 0xC0]); // xor r8d, r8d
        emit_bytes!(&[0x45, 0x31, 0xC9]); // xor r9d, r9d
        emit_bytes!(&[0x0F, 0x05]); // syscall

        emit_bytes!(&[0x49, 0x01, 0xC7]); // add r15, rax
        emit_bytes!(&[0x48, 0x8B, 0x1C, 0x24]); // mov rbx, [rsp]
        emit_bytes!(&[0x48, 0x83, 0xC4, 0x08]); // add rsp, 8
        emit_bytes!(&[0x4C, 0x39, 0xF3]); // cmp rbx, r14
        let jne_fail = cursor;
        emit_bytes!(&[0x75, 0x00]); // jne fail
        emit_bytes!(&[0x49, 0xFF, 0xCE]); // dec r14
        let jnz_loop = cursor;
        emit_bytes!(&[0x75, 0x00]); // jnz loop

        emit_bytes!(&[0x4D, 0x89, 0x3C, 0x24]); // mov [r12], r15
        let success_int3 = cursor;
        emit_bytes!(&[0xCC]); // int3
        emit_bytes!(&[0xEB, 0xFE]); // jmp $

        let fail_label = cursor;
        emit_bytes!(&[0x49, 0xBF]);
        emit_u64!(FAIL_MARKER); // mov r15, FAIL_MARKER
        emit_bytes!(&[0x4D, 0x89, 0x3C, 0x24]); // mov [r12], r15
        emit_bytes!(&[0xCC]); // int3
        emit_bytes!(&[0xEB, 0xFE]); // jmp $
        let _ = cursor;

        let rel_fail = (fail_label as isize) - ((jne_fail + 2) as isize);
        let rel_loop = (loop_start as isize) - ((jnz_loop + 2) as isize);
        if rel_fail < i8::MIN as isize
            || rel_fail > i8::MAX as isize
            || rel_loop < i8::MIN as isize
            || rel_loop > i8::MAX as isize
        {
            shared_value = 0;
            trap_hit = false;
            trap_cs = 0;
            trap_rip = 0;
        } else {
            code_bytes[jne_fail + 1] = rel_fail as i8 as u8;
            code_bytes[jnz_loop + 1] = rel_loop as i8 as u8;
            trap_expected_rip = USER_CODE_VIRT as u64 + success_int3 as u64 + 1;

            arch::x86_64::ring3::clear_saved_resume_rsp();
            arch::x86_64::interrupts::arm_ring3_breakpoint_probe();

            let mut raw_rflags: u64;
            unsafe {
                core::arch::asm!("pushfq", "pop {}", out(reg) raw_rflags, options(nomem, preserves_flags));
            }
            let user_rflags =
                (RFlags::from_bits_truncate(raw_rflags) | RFlags::INTERRUPT_FLAG).bits();

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
        }
    } else {
        shared_value = 0;
        trap_hit = false;
        trap_cs = 0;
        trap_rip = 0;
    }

    serial::write_str("arch: syscall-stress map=");
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
    serial::write_str(" result=");
    serial::write_u64(shared_value);
    serial::write_str(" expected=");
    serial::write_u64(expected_result);
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && trap_hit
        && (trap_cs & 0x3) == 0x3
        && trap_rip == trap_expected_rip
        && shared_value == expected_result;
    serial::write_line(if pass {
        "arch: syscall-stress PASS"
    } else {
        "arch: syscall-stress FAIL"
    });
}
