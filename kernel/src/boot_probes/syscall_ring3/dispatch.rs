use super::*;

static SYSCALL_SIGNAL_TARGET_ID: AtomicU64 = AtomicU64::new(0);

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
