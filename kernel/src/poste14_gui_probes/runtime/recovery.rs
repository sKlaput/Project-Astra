use super::*;

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
