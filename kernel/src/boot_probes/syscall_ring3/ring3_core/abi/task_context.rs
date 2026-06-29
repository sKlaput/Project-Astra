use super::*;

const USER_TASK_CTX_CODE_VIRT: usize = 0x0000_0000_0041_9000;
const USER_TASK_CTX_STACK_VIRT: usize = 0x0000_0000_0041_A000;
const USER_TASK_CTX_SHARED_VIRT: usize = 0x0000_0000_0041_B000;
const USER_TASK_CTX_STACK_TOP: usize = USER_TASK_CTX_STACK_VIRT + memory::paging::PAGE_SIZE - 16;
const USER_TASK_CTX_TRAP_RIP_OFFSET: u64 = 91;

static SYSCALL_TASK_CTX_SHARED_PHYS: AtomicU64 = AtomicU64::new(0);
static SYSCALL_TASK_CTX_EXPECTED_ID: AtomicU64 = AtomicU64::new(0);
static SYSCALL_TASK_CTX_DONE: AtomicU64 = AtomicU64::new(0);
static SYSCALL_TASK_CTX_USER_ID: AtomicU64 = AtomicU64::new(0);
static SYSCALL_TASK_CTX_ADD_RET: AtomicU64 = AtomicU64::new(0);
static SYSCALL_TASK_CTX_ENOSYS_RET: AtomicU64 = AtomicU64::new(0);
static SYSCALL_TASK_CTX_TRAP_HIT: AtomicU64 = AtomicU64::new(0);
static SYSCALL_TASK_CTX_TRAP_CS: AtomicU64 = AtomicU64::new(0);
static SYSCALL_TASK_CTX_TRAP_RIP: AtomicU64 = AtomicU64::new(0);

fn task_syscall_abi_task_context_runner() {
    let self_id = scheduler::current_task().unwrap();
    SYSCALL_TASK_CTX_EXPECTED_ID.store(self_id.0, Ordering::Relaxed);

    let shared_phys = SYSCALL_TASK_CTX_SHARED_PHYS.load(Ordering::Relaxed) as usize;
    if shared_phys == 0 {
        SYSCALL_TASK_CTX_DONE.store(1, Ordering::Relaxed);
        scheduler::exit_task(self_id);
        return;
    }

    let shared_ptr = (shared_phys + memory::paging::hhdm_offset()) as *mut u64;

    arch::x86_64::ring3::clear_saved_resume_rsp();
    arch::x86_64::interrupts::arm_ring3_breakpoint_probe();

    let mut raw_rflags: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) raw_rflags, options(nomem, preserves_flags));
    }
    let user_rflags = (RFlags::from_bits_truncate(raw_rflags) | RFlags::INTERRUPT_FLAG).bits();

    unsafe {
        arch::x86_64::ring3::enter_user_mode(
            USER_TASK_CTX_CODE_VIRT as u64,
            USER_TASK_CTX_STACK_TOP as u64,
            arch::x86_64::ring3_code_selector().0 as u64,
            arch::x86_64::ring3_data_selector().0 as u64,
            user_rflags,
        );
    }

    let user_id = unsafe { core::ptr::read_volatile(shared_ptr) };
    let add_ret = unsafe { core::ptr::read_volatile(shared_ptr.add(1)) };
    let enosys_ret = unsafe { core::ptr::read_volatile(shared_ptr.add(2)) };
    SYSCALL_TASK_CTX_USER_ID.store(user_id, Ordering::Relaxed);
    SYSCALL_TASK_CTX_ADD_RET.store(add_ret, Ordering::Relaxed);
    SYSCALL_TASK_CTX_ENOSYS_RET.store(enosys_ret, Ordering::Relaxed);
    SYSCALL_TASK_CTX_TRAP_HIT.store(
        arch::x86_64::interrupts::ring3_breakpoint_probe_hit() as u64,
        Ordering::Relaxed,
    );
    SYSCALL_TASK_CTX_TRAP_CS.store(
        arch::x86_64::interrupts::ring3_breakpoint_probe_cs(),
        Ordering::Relaxed,
    );
    SYSCALL_TASK_CTX_TRAP_RIP.store(
        arch::x86_64::interrupts::ring3_breakpoint_probe_rip(),
        Ordering::Relaxed,
    );
    arch::x86_64::ring3::clear_saved_resume_rsp();

    SYSCALL_TASK_CTX_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(self_id);
}

pub(crate) fn probe_syscall_abi_task_context() {
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
            memory::paging::map_page_current(USER_TASK_CTX_CODE_VIRT, frame.start_address(), flags)
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
            memory::paging::map_page_current(USER_TASK_CTX_STACK_VIRT, frame.start_address(), flags)
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
            memory::paging::map_page_current(
                USER_TASK_CTX_SHARED_VIRT,
                frame.start_address(),
                flags,
            )
            .is_ok()
        };
    }

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
            core::ptr::write_volatile(shared_ptr.add(1), 0);
            core::ptr::write_volatile(shared_ptr.add(2), 0);
        }

        // mov rbx, USER_TASK_CTX_SHARED_VIRT
        code_bytes[0] = 0x48;
        code_bytes[1] = 0xBB;
        code_bytes[2..10].copy_from_slice(&(USER_TASK_CTX_SHARED_VIRT as u64).to_le_bytes());
        // mov rax, SYS_TASK_ID
        code_bytes[10] = 0x48;
        code_bytes[11] = 0xC7;
        code_bytes[12] = 0xC0;
        code_bytes[13] = 0x05;
        code_bytes[14] = 0x00;
        code_bytes[15] = 0x00;
        code_bytes[16] = 0x00;
        // clear args
        code_bytes[17] = 0x48;
        code_bytes[18] = 0x31;
        code_bytes[19] = 0xFF; // xor rdi,rdi
        code_bytes[20] = 0x48;
        code_bytes[21] = 0x31;
        code_bytes[22] = 0xF6; // xor rsi,rsi
        code_bytes[23] = 0x31;
        code_bytes[24] = 0xD2; // xor edx,edx
        code_bytes[25] = 0x45;
        code_bytes[26] = 0x31;
        code_bytes[27] = 0xD2; // xor r10d,r10d
        code_bytes[28] = 0x45;
        code_bytes[29] = 0x31;
        code_bytes[30] = 0xC0; // xor r8d,r8d
        code_bytes[31] = 0x45;
        code_bytes[32] = 0x31;
        code_bytes[33] = 0xC9; // xor r9d,r9d
                               // syscall
        code_bytes[34] = 0x0F;
        code_bytes[35] = 0x05;
        // mov [rbx], rax
        code_bytes[36] = 0x48;
        code_bytes[37] = 0x89;
        code_bytes[38] = 0x03;
        // mov rax, SYS_ADD
        code_bytes[39] = 0x48;
        code_bytes[40] = 0xC7;
        code_bytes[41] = 0xC0;
        code_bytes[42] = 0x01;
        code_bytes[43] = 0x00;
        code_bytes[44] = 0x00;
        code_bytes[45] = 0x00;
        // mov rdi, 5
        code_bytes[46] = 0x48;
        code_bytes[47] = 0xC7;
        code_bytes[48] = 0xC7;
        code_bytes[49] = 0x05;
        code_bytes[50] = 0x00;
        code_bytes[51] = 0x00;
        code_bytes[52] = 0x00;
        // mov rsi, 6
        code_bytes[53] = 0x48;
        code_bytes[54] = 0xC7;
        code_bytes[55] = 0xC6;
        code_bytes[56] = 0x06;
        code_bytes[57] = 0x00;
        code_bytes[58] = 0x00;
        code_bytes[59] = 0x00;
        // clear remaining args
        code_bytes[60] = 0x31;
        code_bytes[61] = 0xD2;
        code_bytes[62] = 0x45;
        code_bytes[63] = 0x31;
        code_bytes[64] = 0xD2;
        code_bytes[65] = 0x45;
        code_bytes[66] = 0x31;
        code_bytes[67] = 0xC0;
        code_bytes[68] = 0x45;
        code_bytes[69] = 0x31;
        code_bytes[70] = 0xC9;
        // syscall
        code_bytes[71] = 0x0F;
        code_bytes[72] = 0x05;
        // mov [rbx+8], rax
        code_bytes[73] = 0x48;
        code_bytes[74] = 0x89;
        code_bytes[75] = 0x43;
        code_bytes[76] = 0x08;
        // invalid syscall nr=255 -> ENOSYS
        code_bytes[77] = 0x48;
        code_bytes[78] = 0xC7;
        code_bytes[79] = 0xC0;
        code_bytes[80] = 0xFF;
        code_bytes[81] = 0x00;
        code_bytes[82] = 0x00;
        code_bytes[83] = 0x00;
        code_bytes[84] = 0x0F;
        code_bytes[85] = 0x05;
        // mov [rbx+16], rax
        code_bytes[86] = 0x48;
        code_bytes[87] = 0x89;
        code_bytes[88] = 0x43;
        code_bytes[89] = 0x10;
        // int3 + jmp $
        code_bytes[90] = 0xCC;
        code_bytes[91] = 0xEB;
        code_bytes[92] = 0xFE;

        SYSCALL_TASK_CTX_SHARED_PHYS.store(shared_phys as u64, Ordering::Relaxed);
        SYSCALL_TASK_CTX_EXPECTED_ID.store(0, Ordering::Relaxed);
        SYSCALL_TASK_CTX_DONE.store(0, Ordering::Relaxed);
        SYSCALL_TASK_CTX_USER_ID.store(0, Ordering::Relaxed);
        SYSCALL_TASK_CTX_ADD_RET.store(0, Ordering::Relaxed);
        SYSCALL_TASK_CTX_ENOSYS_RET.store(0, Ordering::Relaxed);
        SYSCALL_TASK_CTX_TRAP_HIT.store(0, Ordering::Relaxed);
        SYSCALL_TASK_CTX_TRAP_CS.store(0, Ordering::Relaxed);
        SYSCALL_TASK_CTX_TRAP_RIP.store(0, Ordering::Relaxed);

        let _ = scheduler::spawn_task_with_fn_prio(task_syscall_abi_task_context_runner, 20);
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 160
            && SYSCALL_TASK_CTX_DONE.load(Ordering::Relaxed) == 0
        {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
        }
        while scheduler::dispatch_once() {}
    }

    let done = SYSCALL_TASK_CTX_DONE.load(Ordering::Relaxed) == 1;
    let expected_id = SYSCALL_TASK_CTX_EXPECTED_ID.load(Ordering::Relaxed);
    let user_id = SYSCALL_TASK_CTX_USER_ID.load(Ordering::Relaxed);
    let add_ret = SYSCALL_TASK_CTX_ADD_RET.load(Ordering::Relaxed);
    let enosys_ret = SYSCALL_TASK_CTX_ENOSYS_RET.load(Ordering::Relaxed);
    let trap_hit = SYSCALL_TASK_CTX_TRAP_HIT.load(Ordering::Relaxed) == 1;
    let trap_cs = SYSCALL_TASK_CTX_TRAP_CS.load(Ordering::Relaxed);
    let trap_rip = SYSCALL_TASK_CTX_TRAP_RIP.load(Ordering::Relaxed);

    serial::write_str("arch: syscall-taskctx map=");
    serial::write_u64(map_code as u64);
    serial::write_str(",");
    serial::write_u64(map_stack as u64);
    serial::write_str(",");
    serial::write_u64(map_shared as u64);
    serial::write_str(" done=");
    serial::write_u64(done as u64);
    serial::write_str(" hit=");
    serial::write_u64(trap_hit as u64);
    serial::write_str(" cs=");
    serial::write_u64(trap_cs);
    serial::write_str(" rip=");
    serial::write_u64(trap_rip);
    serial::write_str(" id=");
    serial::write_u64(user_id);
    serial::write_str(" expect=");
    serial::write_u64(expected_id);
    serial::write_str(" add=");
    serial::write_u64(add_ret);
    serial::write_str(" enosys=");
    serial::write_u64(enosys_ret);
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && done
        && trap_hit
        && (trap_cs & 0x3) == 0x3
        && trap_rip == USER_TASK_CTX_CODE_VIRT as u64 + USER_TASK_CTX_TRAP_RIP_OFFSET
        && expected_id != 0
        && user_id == expected_id
        && add_ret == 11
        && enosys_ret == syscall::SYS_ENOSYS;
    serial::write_line(if pass {
        "arch: syscall-taskctx PASS"
    } else {
        "arch: syscall-taskctx FAIL"
    });
}
