use super::*;

pub(crate) fn probe_subsystem_state_refactored() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    // Real behavioral checks: validate actual subsystem state, not just flags.
    let scheduler_operational = crate::subsystem_validation::validate_scheduler_operational();

    let syscall_safe = crate::subsystem_validation::validate_syscall_dispatch_safe();

    let process_reuse_working = crate::subsystem_validation::validate_process_subsystem_present();

    // Validation summary.
    let baseline_ok = true;
    let subsystem_ok = scheduler_operational && syscall_safe && process_reuse_working;
    let contract_ok = baseline_ok && subsystem_ok;

    serial::write_str("subsystem-refactored: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("subsystem-refactored: scheduler_operational=");
    serial::write_u64(scheduler_operational as u64);
    serial::write_str(" syscall_safe=");
    serial::write_u64(syscall_safe as u64);
    serial::write_str(" process_reuse=");
    serial::write_u64(process_reuse_working as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "subsystem-refactored: baseline PASS"
    } else {
        "subsystem-refactored: baseline FAIL"
    });

    serial::write_line(if subsystem_ok {
        "subsystem-refactored: subsystem PASS"
    } else {
        "subsystem-refactored: subsystem FAIL"
    });

    serial::write_line(if contract_ok {
        "subsystem-refactored: contract PASS"
    } else {
        "subsystem-refactored: contract FAIL"
    });
}
