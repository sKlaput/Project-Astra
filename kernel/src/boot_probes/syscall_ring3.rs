use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::registers::rflags::RFlags;

use crate::{arch, idle, loader, memory, scheduler, serial, syscall};

static SYSCALL_SIGNAL_TARGET_ID: AtomicU64 = AtomicU64::new(0);

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

fn task_syscall_signal_target() {
    let self_id = scheduler::current_task().unwrap();
    SYSCALL_SIGNAL_TARGET_ID.store(self_id.0, Ordering::Relaxed);
    scheduler::sleep_current_for_ticks(40);
    scheduler::exit_task(self_id);
}

pub(crate) fn probe_syscall_dispatch() {
    let table_len = syscall::table_len();

    let ticks_before = scheduler::ticks();
    let v_nop = syscall::dispatch(syscall::SYS_NOP, 1, 2, 3, 4, 5, 6);
    let v_add = syscall::dispatch(syscall::SYS_ADD, 7, 35, 0, 0, 0, 0);
    let v_max = syscall::dispatch(syscall::SYS_MAX, 111, 9, 0, 0, 0, 0);
    let v_mix = syscall::dispatch(syscall::SYS_XORROT, 0xA5, 0x5A, 13, 9, 0, 0);
    let v_ticks = syscall::dispatch(syscall::SYS_TICKS, 0, 0, 0, 0, 0, 0);
    let v_task_id = syscall::dispatch(syscall::SYS_TASK_ID, 0, 0, 0, 0, 0, 0);

    SYSCALL_SIGNAL_TARGET_ID.store(0, Ordering::Relaxed);
    let sig_target = scheduler::spawn_task_with_fn_prio(task_syscall_signal_target, 20);
    let start_wait_target = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_wait_target) < 30
        && SYSCALL_SIGNAL_TARGET_ID.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    let signal_target = SYSCALL_SIGNAL_TARGET_ID.load(Ordering::Relaxed);
    let signal_bits = 0x10u64;
    let signal_all_bits = 0x3u64;
    let mask_bits = 0x20u64;
    let v_sig_set = syscall::dispatch(
        syscall::SYS_SIGNAL_SET,
        signal_target,
        signal_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_pending =
        syscall::dispatch(syscall::SYS_SIGNAL_PENDING, signal_target, 0, 0, 0, 0, 0);
    let v_sig_wait = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_UNTIL,
        signal_target,
        signal_bits,
        scheduler::ticks().saturating_add(1),
        0,
        0,
        0,
    );
    let v_sig_wait_inf = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT,
        signal_target,
        signal_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_clear_prev = syscall::dispatch(
        syscall::SYS_SIGNAL_CLEAR,
        signal_target,
        signal_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_pending_after =
        syscall::dispatch(syscall::SYS_SIGNAL_PENDING, signal_target, 0, 0, 0, 0, 0);
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_CLEAR,
        signal_target,
        signal_all_bits,
        0,
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_SET,
        signal_target,
        signal_all_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_wait_all = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_ALL_UNTIL,
        signal_target,
        signal_all_bits,
        scheduler::ticks().saturating_add(1),
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_CLEAR,
        signal_target,
        signal_all_bits,
        0,
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(syscall::SYS_SIGNAL_SET, signal_target, 0x1, 0, 0, 0, 0);
    let v_sig_wait_all_to = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_ALL_UNTIL,
        signal_target,
        signal_all_bits,
        scheduler::ticks(),
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_CLEAR,
        signal_target,
        signal_all_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_mask_get0 =
        syscall::dispatch(syscall::SYS_SIGNAL_MASK_GET, signal_target, 0, 0, 0, 0, 0);
    let v_sig_mask_block_prev = syscall::dispatch(
        syscall::SYS_SIGNAL_BLOCK,
        signal_target,
        mask_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_mask_get1 =
        syscall::dispatch(syscall::SYS_SIGNAL_MASK_GET, signal_target, 0, 0, 0, 0, 0);
    let v_sig_set_blocked = syscall::dispatch(
        syscall::SYS_SIGNAL_SET,
        signal_target,
        mask_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_wait_blocked_to = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_UNTIL,
        signal_target,
        mask_bits,
        scheduler::ticks(),
        0,
        0,
        0,
    );
    let v_sig_mask_unblock_prev = syscall::dispatch(
        syscall::SYS_SIGNAL_UNBLOCK,
        signal_target,
        mask_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_mask_get2 =
        syscall::dispatch(syscall::SYS_SIGNAL_MASK_GET, signal_target, 0, 0, 0, 0, 0);
    let v_sig_wait_unblocked = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_UNTIL,
        signal_target,
        mask_bits,
        scheduler::ticks().saturating_add(1),
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_CLEAR,
        signal_target,
        mask_bits,
        0,
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_CLEAR,
        signal_target,
        signal_bits,
        0,
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_SET,
        signal_target,
        signal_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_consume_until = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_CONSUME_UNTIL,
        signal_target,
        signal_bits,
        scheduler::ticks().saturating_add(1),
        0,
        0,
        0,
    );
    let v_sig_pending_after_consume_until =
        syscall::dispatch(syscall::SYS_SIGNAL_PENDING, signal_target, 0, 0, 0, 0, 0);
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_SET,
        signal_target,
        signal_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_consume_inf = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_CONSUME,
        signal_target,
        signal_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_pending_after_consume_inf =
        syscall::dispatch(syscall::SYS_SIGNAL_PENDING, signal_target, 0, 0, 0, 0, 0);
    let v_sig_consume_to = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_CONSUME_UNTIL,
        signal_target,
        signal_bits,
        scheduler::ticks(),
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_CLEAR,
        signal_target,
        signal_all_bits,
        0,
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_SET,
        signal_target,
        signal_all_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_consume_all_until = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_ALL_CONSUME_UNTIL,
        signal_target,
        signal_all_bits,
        scheduler::ticks().saturating_add(1),
        0,
        0,
        0,
    );
    let v_sig_pending_after_consume_all =
        syscall::dispatch(syscall::SYS_SIGNAL_PENDING, signal_target, 0, 0, 0, 0, 0);
    let _ = syscall::dispatch(syscall::SYS_SIGNAL_SET, signal_target, 0x1, 0, 0, 0, 0);
    let v_sig_consume_all_to = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_ALL_CONSUME_UNTIL,
        signal_target,
        signal_all_bits,
        scheduler::ticks(),
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_CLEAR,
        signal_target,
        signal_all_bits,
        0,
        0,
        0,
        0,
    );
    let _ = syscall::dispatch(
        syscall::SYS_SIGNAL_SET,
        signal_target,
        signal_all_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_consume_all_inf = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_ALL_CONSUME,
        signal_target,
        signal_all_bits,
        0,
        0,
        0,
        0,
    );
    let v_sig_pending_after_consume_all_inf =
        syscall::dispatch(syscall::SYS_SIGNAL_PENDING, signal_target, 0, 0, 0, 0, 0);
    let v_sig_wait_to = syscall::dispatch(
        syscall::SYS_SIGNAL_WAIT_UNTIL,
        signal_target,
        signal_bits,
        scheduler::ticks(),
        0,
        0,
        0,
    );
    let v_sig_bad = syscall::dispatch(
        syscall::SYS_SIGNAL_SET,
        0xFFFF_FFFF_FFFF_FF00,
        1,
        0,
        0,
        0,
        0,
    );

    let v_bad = syscall::dispatch(0xFFFF, 1, 2, 3, 4, 5, 6);
    let ticks_after = scheduler::ticks();

    let exp_mix = (0xA5u64 ^ 0x5Au64).rotate_left(13) ^ 9u64;

    serial::write_str("syscall: table-len=");
    serial::write_u64(table_len);
    serial::write_str(" nop=");
    serial::write_u64(v_nop);
    serial::write_str(" add=");
    serial::write_u64(v_add);
    serial::write_str(" max=");
    serial::write_u64(v_max);
    serial::write_str(" mix=");
    serial::write_u64(v_mix);
    serial::write_str(" ticks=");
    serial::write_u64(v_ticks);
    serial::write_str(" task=");
    serial::write_u64(v_task_id);
    serial::write_str(" sig=");
    serial::write_u64(v_sig_set);
    serial::write_str(",");
    serial::write_u64(v_sig_pending);
    serial::write_str(",");
    serial::write_u64(v_sig_wait);
    serial::write_str(",");
    serial::write_u64(v_sig_wait_inf);
    serial::write_str(",");
    serial::write_u64(v_sig_clear_prev);
    serial::write_str(",");
    serial::write_u64(v_sig_pending_after);
    serial::write_str(",");
    serial::write_u64(v_sig_wait_all);
    serial::write_str(",");
    serial::write_u64(v_sig_wait_all_to);
    serial::write_str(",");
    serial::write_u64(v_sig_mask_get0);
    serial::write_str(",");
    serial::write_u64(v_sig_mask_block_prev);
    serial::write_str(",");
    serial::write_u64(v_sig_mask_get1);
    serial::write_str(",");
    serial::write_u64(v_sig_set_blocked);
    serial::write_str(",");
    serial::write_u64(v_sig_wait_blocked_to);
    serial::write_str(",");
    serial::write_u64(v_sig_mask_unblock_prev);
    serial::write_str(",");
    serial::write_u64(v_sig_mask_get2);
    serial::write_str(",");
    serial::write_u64(v_sig_wait_unblocked);
    serial::write_str(",");
    serial::write_u64(v_sig_consume_until);
    serial::write_str(",");
    serial::write_u64(v_sig_pending_after_consume_until);
    serial::write_str(",");
    serial::write_u64(v_sig_consume_inf);
    serial::write_str(",");
    serial::write_u64(v_sig_pending_after_consume_inf);
    serial::write_str(",");
    serial::write_u64(v_sig_consume_to);
    serial::write_str(",");
    serial::write_u64(v_sig_consume_all_until);
    serial::write_str(",");
    serial::write_u64(v_sig_pending_after_consume_all);
    serial::write_str(",");
    serial::write_u64(v_sig_consume_all_to);
    serial::write_str(",");
    serial::write_u64(v_sig_consume_all_inf);
    serial::write_str(",");
    serial::write_u64(v_sig_pending_after_consume_all_inf);
    serial::write_str(",");
    serial::write_u64(v_sig_wait_to);
    serial::write_str(",");
    serial::write_u64(v_sig_bad);
    serial::write_str(" bad=");
    serial::write_u64(v_bad);
    serial::write_line("");

    // Table grew after E5 to include write_console/yield/exit syscalls.
    // After E8, table expanded to 24 to include SYS_SEND_MSG and SYS_RECV_MSG.
    // After E9 step 2, table expanded to 29 to include framebuffer mapping
    // for user space (MAP_FB) in addition to graphics draw syscalls.
    let pass = table_len == 29
        && v_nop == 0
        && v_add == 42
        && v_max == 111
        && v_mix == exp_mix
        && v_ticks >= ticks_before
        && v_ticks <= ticks_after
        && v_task_id == 0
        && signal_target != 0
        && v_sig_set == 1
        && (v_sig_pending & signal_bits) != 0
        && v_sig_wait == 1
        && v_sig_wait_inf == 1
        && (v_sig_clear_prev & signal_bits) != 0
        && (v_sig_pending_after & signal_bits) == 0
        && v_sig_wait_all == 1
        && v_sig_wait_all_to == 0
        && v_sig_mask_get0 == 0
        && v_sig_mask_block_prev == 0
        && (v_sig_mask_get1 & mask_bits) != 0
        && v_sig_set_blocked == 1
        && v_sig_wait_blocked_to == 0
        && (v_sig_mask_unblock_prev & mask_bits) != 0
        && (v_sig_mask_get2 & mask_bits) == 0
        && v_sig_wait_unblocked == 1
        && v_sig_consume_until == signal_bits
        && (v_sig_pending_after_consume_until & signal_bits) == 0
        && v_sig_consume_inf == signal_bits
        && (v_sig_pending_after_consume_inf & signal_bits) == 0
        && v_sig_consume_to == 0
        && v_sig_consume_all_until == signal_all_bits
        && (v_sig_pending_after_consume_all & signal_all_bits) == 0
        && v_sig_consume_all_to == 0
        && v_sig_consume_all_inf == signal_all_bits
        && (v_sig_pending_after_consume_all_inf & signal_all_bits) == 0
        && v_sig_wait_to == 0
        && v_sig_bad == 0
        && v_bad == syscall::SYS_ENOSYS;

    for _ in 0..80 {
        if let Some(task) = sig_target {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                break;
            }
        }
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    serial::write_line(if pass {
        "syscall: dispatch PASS"
    } else {
        "syscall: dispatch FAIL"
    });
}

pub(crate) fn probe_ring3_descriptors() {
    // Validate ring-3 GDT descriptors are present and have correct privilege bits.
    let code_sel = arch::x86_64::ring3_code_selector();
    let data_sel = arch::x86_64::ring3_data_selector();

    // Selectors should be non-zero.
    let code_valid = code_sel.0 != 0;
    let data_valid = data_sel.0 != 0;

    // RPL bits (bit 1-0) should be 3 for ring-3 ring privilege level.
    let code_rpl = (code_sel.0 & 0x3) as u8;
    let data_rpl = (data_sel.0 & 0x3) as u8;
    let code_rpl_ok = code_rpl == 3;
    let data_rpl_ok = data_rpl == 3;

    serial::write_str("arch: ring3-descriptors code_sel=");
    serial::write_u64(code_sel.0 as u64);
    serial::write_str(" data_sel=");
    serial::write_u64(data_sel.0 as u64);
    serial::write_str(" code_valid=");
    serial::write_u64(code_valid as u64);
    serial::write_str(" data_valid=");
    serial::write_u64(data_valid as u64);
    serial::write_str(" code_rpl=");
    serial::write_u64(code_rpl as u64);
    serial::write_str(" data_rpl=");
    serial::write_u64(data_rpl as u64);
    serial::write_line("");

    let pass = code_valid && data_valid && code_rpl_ok && data_rpl_ok;
    serial::write_line(if pass {
        "arch: ring3-descriptors PASS"
    } else {
        "arch: ring3-descriptors FAIL"
    });
}

pub(crate) fn probe_syscall_entry_msrs() {
    let kernel_cs = arch::x86_64::kernel_code_selector().0 as u64;
    let kernel_ss = arch::x86_64::kernel_data_selector().0 as u64;
    let user_cs = arch::x86_64::ring3_code_selector().0 as u64;
    let user_ss = arch::x86_64::ring3_data_selector().0 as u64;

    let efer = arch::x86_64::sysentry::efer();
    let star = arch::x86_64::sysentry::star();
    let lstar = arch::x86_64::sysentry::lstar();
    let fmask = arch::x86_64::sysentry::fmask();
    let stub = arch::x86_64::sysentry::syscall_entry_addr();

    let efer_sce = (efer & 1) != 0;
    let star_kernel = (star >> 32) & 0xffff;
    let star_user_base = (star >> 48) & 0xffff;
    let sysret_ss = star_user_base + 8;
    let sysret_cs = star_user_base + 16;
    let fmask_if = (fmask & (1 << 9)) != 0;

    serial::write_str("arch: syscall-msr efer_sce=");
    serial::write_u64(efer_sce as u64);
    serial::write_str(" kcs=");
    serial::write_u64(star_kernel);
    serial::write_str(" kss=");
    serial::write_u64(kernel_ss);
    serial::write_str(" ucs=");
    serial::write_u64(sysret_cs);
    serial::write_str(" uss=");
    serial::write_u64(sysret_ss);
    serial::write_str(" lstar_ok=");
    serial::write_u64((lstar == stub) as u64);
    serial::write_str(" fmask=");
    serial::write_u64(fmask);
    serial::write_str(" fmask_if=");
    serial::write_u64(fmask_if as u64);
    serial::write_line("");

    let pass = efer_sce
        && star_kernel == kernel_cs
        && kernel_ss == kernel_cs + 8
        && sysret_cs == user_cs
        && sysret_ss == user_ss
        && lstar == stub
        && fmask == (1 << 9)
        && fmask_if;
    serial::write_line(if pass {
        "arch: syscall-msr PASS"
    } else {
        "arch: syscall-msr FAIL"
    });
}

pub(crate) fn probe_ring3_user_mapping() {
    const USER_CODE_VIRT: usize = 0x0000_0000_0040_0000;
    const USER_STACK_VIRT: usize = 0x0000_0000_0040_1000;
    const USER_SHARED_VIRT: usize = 0x0000_0000_0040_2000;

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

    let code_entry =
        unsafe { memory::paging::lookup_page_entry_current(USER_CODE_VIRT).unwrap_or(0) };
    let stack_entry =
        unsafe { memory::paging::lookup_page_entry_current(USER_STACK_VIRT).unwrap_or(0) };
    let shared_entry =
        unsafe { memory::paging::lookup_page_entry_current(USER_SHARED_VIRT).unwrap_or(0) };

    let code_user = (code_entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0;
    let code_write = (code_entry & memory::paging::PageTableFlags::WRITABLE) != 0;
    let stack_user = (stack_entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0;
    let stack_write = (stack_entry & memory::paging::PageTableFlags::WRITABLE) != 0;
    let shared_user = (shared_entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0;
    let shared_write = (shared_entry & memory::paging::PageTableFlags::WRITABLE) != 0;

    serial::write_str("arch: ring3-map code=");
    serial::write_u64(map_code as u64);
    serial::write_str(",");
    serial::write_u64(code_user as u64);
    serial::write_str(",");
    serial::write_u64(code_write as u64);
    serial::write_str(" stack=");
    serial::write_u64(map_stack as u64);
    serial::write_str(",");
    serial::write_u64(stack_user as u64);
    serial::write_str(",");
    serial::write_u64(stack_write as u64);
    serial::write_str(" shared=");
    serial::write_u64(map_shared as u64);
    serial::write_str(",");
    serial::write_u64(shared_user as u64);
    serial::write_str(",");
    serial::write_u64(shared_write as u64);
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && code_user
        && !code_write
        && stack_user
        && stack_write
        && shared_user
        && shared_write;
    serial::write_line(if pass {
        "arch: ring3-map PASS"
    } else {
        "arch: ring3-map FAIL"
    });
}

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
