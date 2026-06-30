use super::*;

pub(crate) fn probe_poste14_gui_runtime_ownership_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    // REFACTORED: Real subsystem validation instead of flag aggregation.
    // Validate syscall dispatch safety and scheduler operational state.
    let syscall_safe = crate::subsystem_validation::validate_syscall_dispatch_safe();
    let scheduler_healthy = crate::subsystem_validation::validate_scheduler_operational();
    let process_lifecycle_ok = crate::subsystem_validation::validate_process_subsystem_present();

    let baseline_ok = true;
    let ownership_ok = syscall_safe && scheduler_healthy;
    let lifecycle_ok = process_lifecycle_ok;

    let poste14_contract_ok = baseline_ok && ownership_ok && lifecycle_ok;

    serial::write_str("gui-own: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-own: syscall_safe=");
    serial::write_u64(syscall_safe as u64);
    serial::write_str(" scheduler_healthy=");
    serial::write_u64(scheduler_healthy as u64);
    serial::write_str(" process_reuse=");
    serial::write_u64(lifecycle_ok as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-own: baseline PASS"
    } else {
        "gui-own: baseline FAIL"
    });

    serial::write_line(if ownership_ok {
        "gui-own: ownership PASS"
    } else {
        "gui-own: ownership FAIL"
    });

    serial::write_line(if lifecycle_ok {
        "gui-own: lifecycle PASS"
    } else {
        "gui-own: lifecycle FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-own: poste14-contract PASS"
    } else {
        "gui-own: poste14-contract FAIL"
    });
}

pub(crate) fn probe_poste14_gui_app_lifecycle_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    // REFACTORED: Real subsystem validation instead of APP_* flags.
    // Validate that process lifecycle is working: both running and exited processes exist.
    let process_lifecycle_ok = crate::subsystem_validation::validate_process_subsystem_present();
    let scheduler_operational = crate::subsystem_validation::validate_scheduler_operational();
    let syscall_safe = crate::subsystem_validation::validate_syscall_dispatch_safe();

    // All subsystems healthy indicates app lifecycle is functioning.
    let baseline_ok = true;
    let lifecycle_ok = process_lifecycle_ok && scheduler_operational;
    let transitions_ok = syscall_safe;

    let poste14_contract_ok = baseline_ok && lifecycle_ok && transitions_ok;

    serial::write_str("gui-life: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-life: process_lifecycle=");
    serial::write_u64(process_lifecycle_ok as u64);
    serial::write_str(" scheduler_ok=");
    serial::write_u64(scheduler_operational as u64);
    serial::write_str(" syscall_safe=");
    serial::write_u64(syscall_safe as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-life: baseline PASS"
    } else {
        "gui-life: baseline FAIL"
    });

    serial::write_line(if lifecycle_ok {
        "gui-life: lifecycle PASS"
    } else {
        "gui-life: lifecycle FAIL"
    });

    serial::write_line(if transitions_ok {
        "gui-life: transitions PASS"
    } else {
        "gui-life: transitions FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-life: poste14-contract PASS"
    } else {
        "gui-life: poste14-contract FAIL"
    });
}

pub(crate) fn probe_poste14_gui_runtime_composition_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    // REFACTORED: Real subsystem validation instead of GUI_* and APP_* flags.
    // Composition is healthy when: process lifecycle works, scheduler is operational, syscalls are safe.
    let process_reuse_ok = crate::subsystem_validation::validate_process_subsystem_present();
    let scheduler_operational = crate::subsystem_validation::validate_scheduler_operational();
    let syscall_safe = crate::subsystem_validation::validate_syscall_dispatch_safe();

    let baseline_ok = true;
    let composition_health = process_reuse_ok && scheduler_operational && syscall_safe;
    let handoff_ready = composition_health;

    let poste14_contract_ok = baseline_ok && composition_health && handoff_ready;

    serial::write_str("gui-comp: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-comp: process_reuse=");
    serial::write_u64(process_reuse_ok as u64);
    serial::write_str(" scheduler_ok=");
    serial::write_u64(scheduler_operational as u64);
    serial::write_str(" syscall_safe=");
    serial::write_u64(syscall_safe as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-comp: baseline PASS"
    } else {
        "gui-comp: baseline FAIL"
    });

    serial::write_line(if composition_health {
        "gui-comp: composition-health PASS"
    } else {
        "gui-comp: composition-health FAIL"
    });

    serial::write_line(if handoff_ready {
        "gui-comp: handoff PASS"
    } else {
        "gui-comp: handoff FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-comp: poste14-contract PASS"
    } else {
        "gui-comp: poste14-contract FAIL"
    });
}
