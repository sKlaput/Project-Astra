use super::*;

// ---------------------------------------------------------------------------
// probe_syscall_sysret_roundtrip
//
// Maps three user pages (code / stack / shared), writes machine code that:
//   1. Invokes `syscall` with SYS_ADD(7, 35) — expects result 42 in RAX
//   2. Stores RAX to shared memory
//   3. Executes `int3` to return to kernel context
//
// User code bytes (Intel / little-endian):
//   [0..9]  48 B8 01 00 00 00 00 00 00 00  ; mov rax, 1  (SYS_ADD)
//   [10..19] 48 BF 07 00 00 00 00 00 00 00 ; mov rdi, 7
//   [20..29] 48 BE 23 00 00 00 00 00 00 00 ; mov rsi, 35
//   [30..31] 31 D2                          ; xor edx, edx
//   [32..34] 45 31 D2                       ; xor r10d, r10d
//   [35..37] 45 31 C0                       ; xor r8d, r8d
//   [38..40] 45 31 C9                       ; xor r9d, r9d
//   [41..42] 0F 05                          ; syscall
//   [43..52] 48 BB ?? ?? ?? ?? ?? ?? ?? ??  ; mov rbx, USER_SHARED_VIRT
//   [53..55] 48 89 03                       ; mov [rbx], rax
//   [56]     CC                             ; int3
//   [57..58] EB FE                          ; jmp $ (spin)
//
// After the roundtrip the probe verifies:
//   shared_value == 42  (correct syscall result)
//   trap_cs & 3 == 3    (breakpoint fired from CPL3)
//   trap_rip == code_va + 57 (one past int3)
// ---------------------------------------------------------------------------
pub(crate) fn probe_syscall_sysret_roundtrip() {
    const USER_CODE_VIRT: usize = 0x0000_0000_0041_0000;
    const USER_STACK_VIRT: usize = 0x0000_0000_0041_1000;
    const USER_SHARED_VIRT: usize = 0x0000_0000_0041_2000;
    const USER_STACK_TOP: usize = USER_STACK_VIRT + memory::paging::PAGE_SIZE - 16;
    const EXPECTED_RESULT: u64 = 42; // SYS_ADD(7, 35)
    const USER_TRAP_RIP_OFFSET: u64 = 57; // byte after int3 at offset 56

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

        // mov rax, 1  (SYS_ADD)
        code_bytes[0] = 0x48;
        code_bytes[1] = 0xB8;
        code_bytes[2..10].copy_from_slice(&1u64.to_le_bytes());
        // mov rdi, 7
        code_bytes[10] = 0x48;
        code_bytes[11] = 0xBF;
        code_bytes[12..20].copy_from_slice(&7u64.to_le_bytes());
        // mov rsi, 35
        code_bytes[20] = 0x48;
        code_bytes[21] = 0xBE;
        code_bytes[22..30].copy_from_slice(&35u64.to_le_bytes());
        // xor edx, edx
        code_bytes[30] = 0x31;
        code_bytes[31] = 0xD2;
        // xor r10d, r10d
        code_bytes[32] = 0x45;
        code_bytes[33] = 0x31;
        code_bytes[34] = 0xD2;
        // xor r8d, r8d
        code_bytes[35] = 0x45;
        code_bytes[36] = 0x31;
        code_bytes[37] = 0xC0;
        // xor r9d, r9d
        code_bytes[38] = 0x45;
        code_bytes[39] = 0x31;
        code_bytes[40] = 0xC9;
        // syscall
        code_bytes[41] = 0x0F;
        code_bytes[42] = 0x05;
        // mov rbx, USER_SHARED_VIRT
        code_bytes[43] = 0x48;
        code_bytes[44] = 0xBB;
        code_bytes[45..53].copy_from_slice(&(USER_SHARED_VIRT as u64).to_le_bytes());
        // mov [rbx], rax
        code_bytes[53] = 0x48;
        code_bytes[54] = 0x89;
        code_bytes[55] = 0x03;
        // int3
        code_bytes[56] = 0xCC;
        // jmp $ (spin)
        code_bytes[57] = 0xEB;
        code_bytes[58] = 0xFE;

        arch::x86_64::ring3::clear_saved_resume_rsp();
        arch::x86_64::interrupts::arm_ring3_breakpoint_probe();

        let mut raw_rflags: u64;
        unsafe {
            core::arch::asm!(
                "pushfq", "pop {}", out(reg) raw_rflags,
                options(nomem, preserves_flags)
            );
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

    serial::write_str("arch: syscall-sysret map=");
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
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && trap_hit
        && (trap_cs & 0x3) == 0x3
        && trap_rip == USER_CODE_VIRT as u64 + USER_TRAP_RIP_OFFSET
        && shared_value == EXPECTED_RESULT;
    serial::write_line(if pass {
        "arch: syscall-sysret PASS"
    } else {
        "arch: syscall-sysret FAIL"
    });
}
