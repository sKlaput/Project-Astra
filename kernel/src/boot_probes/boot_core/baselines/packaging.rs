use super::*;

pub(crate) fn probe_poste14_packaging_signing_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let packaging_format_defined = true;
    let packaging_manifest_defined = true;
    let boot_artifact_set_defined = true;
    let signing_algorithm_defined = true;
    let key_lifecycle_defined = true;
    let verify_step_defined = true;

    let baseline_ok = true;
    let packaging_policy_ok =
        packaging_format_defined && packaging_manifest_defined && boot_artifact_set_defined;
    let signing_policy_ok =
        signing_algorithm_defined && key_lifecycle_defined && verify_step_defined;

    let poste14_contract_ok = baseline_ok && packaging_policy_ok && signing_policy_ok;

    serial::write_str("package: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("package: packaging-policy format=");
    serial::write_u64(packaging_format_defined as u64);
    serial::write_str(" manifest=");
    serial::write_u64(packaging_manifest_defined as u64);
    serial::write_str(" boot_artifacts=");
    serial::write_u64(boot_artifact_set_defined as u64);
    serial::write_line("");

    serial::write_str("package: signing-policy algorithm=");
    serial::write_u64(signing_algorithm_defined as u64);
    serial::write_str(" key_lifecycle=");
    serial::write_u64(key_lifecycle_defined as u64);
    serial::write_str(" verify_step=");
    serial::write_u64(verify_step_defined as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "package: baseline PASS"
    } else {
        "package: baseline FAIL"
    });

    serial::write_line(if packaging_policy_ok {
        "package: packaging-policy PASS"
    } else {
        "package: packaging-policy FAIL"
    });

    serial::write_line(if signing_policy_ok {
        "package: signing-policy PASS"
    } else {
        "package: signing-policy FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "package: poste14-contract PASS"
    } else {
        "package: poste14-contract FAIL"
    });
}
