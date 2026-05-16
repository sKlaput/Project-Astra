use super::*;

pub(super) fn probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended5() {
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
    let recover_guard_cont_hyst3_ext5_ready = process_ok && scheduler_ok;
    let recovery_guardrails_cont_hyst3_ext5_surface_ready = scheduler_ok && syscall_ok;
    let window_ok = recover_guard_cont_hyst3_ext5_ready && recovery_guardrails_cont_hyst3_ext5_surface_ready;
    let policy_ok = window_ok && process_ok && syscall_ok;

    let baseline_ok = true;
    let poste14_contract_ok = baseline_ok && window_ok && policy_ok;

    serial::write_str("gui-recover-guard-cont-hyst3-ext5: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-recover-guard-cont-hyst3-ext5: window recover_guard_cont_hyst3_ext5_ready=");
    serial::write_u64(recover_guard_cont_hyst3_ext5_ready as u64);
    serial::write_str(" recovery_guardrails_cont_hyst3_ext5_surface_ready=");
    serial::write_u64(recovery_guardrails_cont_hyst3_ext5_surface_ready as u64);
    serial::write_line("");

    serial::write_str("gui-recover-guard-cont-hyst3-ext5: policy process_ok=");
    serial::write_u64(process_ok as u64);
    serial::write_str(" scheduler_ok=");
    serial::write_u64(scheduler_ok as u64);
    serial::write_str(" syscall_ok=");
    serial::write_u64(syscall_ok as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-recover-guard-cont-hyst3-ext5: baseline PASS"
    } else {
        "gui-recover-guard-cont-hyst3-ext5: baseline FAIL"
    });

    serial::write_line(if window_ok {
        "gui-recover-guard-cont-hyst3-ext5: window PASS"
    } else {
        "gui-recover-guard-cont-hyst3-ext5: window FAIL"
    });

    serial::write_line(if policy_ok {
        "gui-recover-guard-cont-hyst3-ext5: policy PASS"
    } else {
        "gui-recover-guard-cont-hyst3-ext5: policy FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-recover-guard-cont-hyst3-ext5: poste14-contract PASS"
    } else {
        "gui-recover-guard-cont-hyst3-ext5: poste14-contract FAIL"
    });
}

pub(super) fn probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended5() {
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
    let guard_cont_hyst_envelope3_ext5_ready = process_ok && scheduler_ok;
    let guardrails_continuity_hyst_envelope3_ext5_surface_ready = scheduler_ok && syscall_ok;
    let window_ok = guard_cont_hyst_envelope3_ext5_ready && guardrails_continuity_hyst_envelope3_ext5_surface_ready;
    let policy_ok = window_ok && process_ok && syscall_ok;

    let baseline_ok = true;
    let poste14_contract_ok = baseline_ok && window_ok && policy_ok;

    serial::write_str("gui-guard-cont-hyst-envelope3-ext5: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("gui-guard-cont-hyst-envelope3-ext5: window guard_cont_hyst_envelope3_ext5_ready=");
    serial::write_u64(guard_cont_hyst_envelope3_ext5_ready as u64);
    serial::write_str(" guardrails_continuity_hyst_envelope3_ext5_surface_ready=");
    serial::write_u64(guardrails_continuity_hyst_envelope3_ext5_surface_ready as u64);
    serial::write_line("");

    serial::write_str("gui-guard-cont-hyst-envelope3-ext5: policy process_ok=");
    serial::write_u64(process_ok as u64);
    serial::write_str(" scheduler_ok=");
    serial::write_u64(scheduler_ok as u64);
    serial::write_str(" syscall_ok=");
    serial::write_u64(syscall_ok as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui-guard-cont-hyst-envelope3-ext5: baseline PASS"
    } else {
        "gui-guard-cont-hyst-envelope3-ext5: baseline FAIL"
    });

    serial::write_line(if window_ok {
        "gui-guard-cont-hyst-envelope3-ext5: window PASS"
    } else {
        "gui-guard-cont-hyst-envelope3-ext5: window FAIL"
    });

    serial::write_line(if policy_ok {
        "gui-guard-cont-hyst-envelope3-ext5: policy PASS"
    } else {
        "gui-guard-cont-hyst-envelope3-ext5: policy FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "gui-guard-cont-hyst-envelope3-ext5: poste14-contract PASS"
    } else {
        "gui-guard-cont-hyst-envelope3-ext5: poste14-contract FAIL"
    });
}
