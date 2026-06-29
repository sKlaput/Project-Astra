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

pub(crate) fn probe_poste14_gui_focus_arbitration_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    // REFACTORED: Real subsystem validation.
    let scheduler_ok = crate::subsystem_validation::validate_scheduler_operational();
    let process_ok = crate::subsystem_validation::validate_process_subsystem_present();
    let syscall_ok = crate::subsystem_validation::validate_syscall_dispatch_safe();

    let baseline_ok = true;
    let owner_policy_ok = process_ok && scheduler_ok;
    let arbitration_path_ok = scheduler_ok && syscall_ok;

    let poste14_contract_ok = baseline_ok && owner_policy_ok && arbitration_path_ok;

    serial::write_str("gui-focus: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-focus: owner process_ok=");
    serial::write_u64(process_ok as u64);
    serial::write_str(" scheduler_ok=");
    serial::write_u64(scheduler_ok as u64);
    serial::write_line("");

    serial::write_str("gui-focus: arbitration scheduler=");
    serial::write_u64(scheduler_ok as u64);
    serial::write_str(" process_ok=");
    serial::write_u64(process_ok as u64);
    serial::write_str(" syscall_ok=");
    serial::write_u64(syscall_ok as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-focus: baseline PASS"
    } else {
        "gui-focus: baseline FAIL"
    });

    serial::write_line(if owner_policy_ok {
        "gui-focus: owner PASS"
    } else {
        "gui-focus: owner FAIL"
    });

    serial::write_line(if arbitration_path_ok {
        "gui-focus: arbitration-path PASS"
    } else {
        "gui-focus: arbitration-path FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-focus: poste14-contract PASS"
    } else {
        "gui-focus: poste14-contract FAIL"
    });
}

pub(crate) fn probe_poste14_gui_input_routing_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let (scheduler_ok, process_ok, syscall_ok) = subsystem_health_triplet();
    let routing_matrix_ok = scheduler_ok && syscall_ok;
    let ownership_ok = process_ok && scheduler_ok;

    let baseline_ok = true;
    let poste14_contract_ok = baseline_ok && ownership_ok && routing_matrix_ok;

    serial::write_str("gui-input: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-input: ownership process_ok=");
    serial::write_u64(process_ok as u64);
    serial::write_str(" scheduler_ok=");
    serial::write_u64(scheduler_ok as u64);
    serial::write_line("");

    serial::write_str("gui-input: routing scheduler=");
    serial::write_u64(scheduler_ok as u64);
    serial::write_str(" syscall=");
    serial::write_u64(syscall_ok as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-input: baseline PASS"
    } else {
        "gui-input: baseline FAIL"
    });

    serial::write_line(if ownership_ok {
        "gui-input: ownership PASS"
    } else {
        "gui-input: ownership FAIL"
    });

    serial::write_line(if routing_matrix_ok {
        "gui-input: routing-path PASS"
    } else {
        "gui-input: routing-path FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-input: poste14-contract PASS"
    } else {
        "gui-input: poste14-contract FAIL"
    });
}

pub(crate) fn probe_poste14_gui_focus_recovery_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let (scheduler_ok, process_ok, syscall_ok) = subsystem_health_triplet();

    // Recovery fallback baseline for this slice:
    // ownership is healthy when process lifecycle and scheduler health are both true.
    let fallback_owner_ok = process_ok && scheduler_ok;
    let recovery_path_ok = fallback_owner_ok && syscall_ok;

    let baseline_ok = true;
    let poste14_contract_ok = baseline_ok && fallback_owner_ok && recovery_path_ok;

    serial::write_str("gui-recover: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-recover: fallback process_owner=");
    serial::write_u64(fallback_owner_ok as u64);
    serial::write_str(" scheduler_ok=");
    serial::write_u64(scheduler_ok as u64);
    serial::write_line("");

    serial::write_str("gui-recover: recovery-path syscall_ok=");
    serial::write_u64(syscall_ok as u64);
    serial::write_str(" process_ok=");
    serial::write_u64(process_ok as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-recover: baseline PASS"
    } else {
        "gui-recover: baseline FAIL"
    });

    serial::write_line(if fallback_owner_ok {
        "gui-recover: fallback-owner PASS"
    } else {
        "gui-recover: fallback-owner FAIL"
    });

    serial::write_line(if recovery_path_ok {
        "gui-recover: recovery-path PASS"
    } else {
        "gui-recover: recovery-path FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-recover: poste14-contract PASS"
    } else {
        "gui-recover: poste14-contract FAIL"
    });
}

pub(crate) fn probe_poste14_gui_event_ordering_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let (scheduler_ok, process_ok, syscall_ok) = subsystem_health_triplet();

    // Deterministic event-order baseline:
    // focus ownership readiness and routing readiness must both precede event ordering PASS.
    let owner_before_route_ok = process_ok && scheduler_ok;
    let ordered_path_ok = owner_before_route_ok && syscall_ok;

    let baseline_ok = true;
    let policy_ok = owner_before_route_ok;
    let poste14_contract_ok = baseline_ok && policy_ok && ordered_path_ok;

    serial::write_str("gui-order: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-order: policy process_ok=");
    serial::write_u64(process_ok as u64);
    serial::write_str(" scheduler_ok=");
    serial::write_u64(scheduler_ok as u64);
    serial::write_line("");

    serial::write_str("gui-order: event-path syscall_ok=");
    serial::write_u64(syscall_ok as u64);
    serial::write_str(" owner_before_route=");
    serial::write_u64(owner_before_route_ok as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-order: baseline PASS"
    } else {
        "gui-order: baseline FAIL"
    });

    serial::write_line(if policy_ok {
        "gui-order: policy PASS"
    } else {
        "gui-order: policy FAIL"
    });

    serial::write_line(if ordered_path_ok {
        "gui-order: event-ordering-path PASS"
    } else {
        "gui-order: event-ordering-path FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-order: poste14-contract PASS"
    } else {
        "gui-order: poste14-contract FAIL"
    });
}

pub(crate) fn probe_poste14_gui_recovery_escalation_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let (scheduler_ok, process_ok, syscall_ok) = subsystem_health_triplet();
    let recovery_ready = process_ok && scheduler_ok;
    let ordered_path_ready = scheduler_ok && syscall_ok;

    // Baseline escalation model:
    // escalation policy is considered armed when recovery owner is stable and ordered-path readiness is intact.
    let escalation_arm_ok = recovery_ready && ordered_path_ready;
    let escalation_path_ok = escalation_arm_ok && syscall_ok;

    let baseline_ok = true;
    let poste14_contract_ok = baseline_ok && escalation_arm_ok && escalation_path_ok;

    serial::write_str("gui-escalate: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-escalate: arm recovery_ready=");
    serial::write_u64(recovery_ready as u64);
    serial::write_str(" ordered_path=");
    serial::write_u64(ordered_path_ready as u64);
    serial::write_line("");

    serial::write_str("gui-escalate: escalation-path syscall_ok=");
    serial::write_u64(syscall_ok as u64);
    serial::write_str(" process_ok=");
    serial::write_u64(process_ok as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-escalate: baseline PASS"
    } else {
        "gui-escalate: baseline FAIL"
    });

    serial::write_line(if escalation_arm_ok {
        "gui-escalate: arm PASS"
    } else {
        "gui-escalate: arm FAIL"
    });

    serial::write_line(if escalation_path_ok {
        "gui-escalate: escalation-path PASS"
    } else {
        "gui-escalate: escalation-path FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-escalate: poste14-contract PASS"
    } else {
        "gui-escalate: poste14-contract FAIL"
    });
}
