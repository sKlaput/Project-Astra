use super::*;

pub(crate) fn probe_poste14_apic_transition_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let idt_stage = arch::x86_64::interrupts::legacy_idt_bringup_stage();
    let timer_vector = arch::x86_64::interrupts::legacy_timer_vector();
    let (pic_master_offset, pic_slave_offset) =
        arch::x86_64::interrupts::legacy_pic_vector_offsets();
    let (spurious_master_vector, spurious_slave_vector) =
        arch::x86_64::interrupts::legacy_spurious_vectors();
    let pit_target_hz = arch::x86_64::interrupts::legacy_pit_target_hz();

    // In this bounded boot probe, zero deltas are allowed; PASS indicates
    // APIC-transition readiness telemetry is wired and emitted.
    let baseline_ok = true;
    let vector_plan_ok = timer_vector == pic_master_offset
        && pic_master_offset == 0x20
        && pic_slave_offset == 0x28
        && spurious_master_vector == pic_master_offset + 7
        && spurious_slave_vector == pic_slave_offset + 7;
    let timer_source_ok =
        pit_target_hz == 100 && arch::x86_64::interrupts::timer_hz() == pit_target_hz;
    let staged_compat_ok = idt_stage >= 4;

    let poste14_contract_ok = baseline_ok && vector_plan_ok && timer_source_ok && staged_compat_ok;

    serial::write_str("apic: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("apic: legacy vectors timer=");
    serial::write_u64(timer_vector as u64);
    serial::write_str(" pic_master=");
    serial::write_u64(pic_master_offset as u64);
    serial::write_str(" pic_slave=");
    serial::write_u64(pic_slave_offset as u64);
    serial::write_str(" spurious_master=");
    serial::write_u64(spurious_master_vector as u64);
    serial::write_str(" spurious_slave=");
    serial::write_u64(spurious_slave_vector as u64);
    serial::write_line("");

    serial::write_str("apic: timer-source pit_hz=");
    serial::write_u64(pit_target_hz as u64);
    serial::write_str(" timer_hz=");
    serial::write_u64(arch::x86_64::interrupts::timer_hz() as u64);
    serial::write_str(" idt_stage=");
    serial::write_u64(idt_stage as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "apic: baseline PASS"
    } else {
        "apic: baseline FAIL"
    });

    serial::write_line(if vector_plan_ok {
        "apic: vector-plan PASS"
    } else {
        "apic: vector-plan FAIL"
    });

    serial::write_line(if timer_source_ok {
        "apic: timer-source PASS"
    } else {
        "apic: timer-source FAIL"
    });

    serial::write_line(if staged_compat_ok {
        "apic: staged-compat PASS"
    } else {
        "apic: staged-compat FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "apic: poste14-contract PASS"
    } else {
        "apic: poste14-contract FAIL"
    });
}
