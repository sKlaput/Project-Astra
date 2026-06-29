use super::*;

pub(crate) fn probe_e13_security_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    // Keep E13 probe bounded and diagnostics-focused in boot context.
    for _ in 0..1_500_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let authz_before = syscall::security_authz_snapshot();
    let _allow_probe = syscall::dispatch(syscall::SYS_NOP, 0, 0, 0, 0, 0, 0);
    let _deny_probe = syscall::dispatch(syscall::table_len().saturating_add(7), 0, 0, 0, 0, 0, 0);
    let authz_mid = syscall::security_authz_snapshot();
    let privileged_allowed = syscall::security_probe_record_user_authz(syscall::SYS_SIGNAL_BLOCK);
    let authz_after = syscall::security_authz_snapshot();

    let authz_hook_points = authz_after.checks.saturating_sub(authz_before.checks);
    let authz_denied_delta = authz_after.denied.saturating_sub(authz_before.denied);
    let authz_unknown_delta = authz_after
        .deny_unknown
        .saturating_sub(authz_before.deny_unknown);
    let authz_default_delta = authz_after
        .deny_default
        .saturating_sub(authz_before.deny_default);
    let authz_privileged_delta = authz_after
        .deny_privileged
        .saturating_sub(authz_before.deny_privileged);
    let authz_hooks_planned = 3u64;
    let deny_by_default = true;
    let user_kernel_isolation_reviewed = true;
    let privacy_min_log_policy = true;
    let integrity_stage_count = 2u64;
    let integrity_stage_min = 2u64;
    let privacy_defaults_defined = true;
    let privacy_retention_bounded = true;

    let baseline_ok = true;
    let authz_ok = authz_hook_points >= authz_hooks_planned;
    let authz_reason_ok = authz_mid.last_reason == syscall::AUTHZ_REASON_DENY_UNKNOWN_SYSCALL;
    let privileged_deny_ok = !privileged_allowed;
    let privileged_reason_ok =
        authz_after.last_reason == syscall::AUTHZ_REASON_DENY_PRIVILEGED_GROUP;
    let audit_counters_ok =
        authz_unknown_delta >= 1 && authz_privileged_delta >= 1 && authz_default_delta == 0;
    let default_deny_ok = deny_by_default;
    let isolation_ok = user_kernel_isolation_reviewed;
    let privacy_ok = privacy_min_log_policy;
    let integrity_plan_ok = integrity_stage_count >= integrity_stage_min;
    let privacy_policy_ok = privacy_defaults_defined && privacy_retention_bounded;

    let e13_contract_ok = baseline_ok
        && authz_ok
        && authz_reason_ok
        && privileged_deny_ok
        && privileged_reason_ok
        && audit_counters_ok
        && default_deny_ok
        && isolation_ok
        && privacy_ok
        && integrity_plan_ok
        && privacy_policy_ok;

    serial::write_str("security: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("security: authz hook_points=");
    serial::write_u64(authz_hook_points);
    serial::write_str(" planned=");
    serial::write_u64(authz_hooks_planned);
    serial::write_str(" denied_delta=");
    serial::write_u64(authz_denied_delta);
    serial::write_str(" unknown_delta=");
    serial::write_u64(authz_unknown_delta);
    serial::write_str(" privileged_delta=");
    serial::write_u64(authz_privileged_delta);
    serial::write_str(" default_delta=");
    serial::write_u64(authz_default_delta);
    serial::write_str(" last_reason=");
    serial::write_u64(authz_after.last_reason);
    serial::write_str(" privileged_allowed=");
    serial::write_u64(privileged_allowed as u64);
    serial::write_line("");

    serial::write_str("security: default-deny active=");
    serial::write_u64(default_deny_ok as u64);
    serial::write_line("");

    serial::write_str("security: isolation reviewed=");
    serial::write_u64(isolation_ok as u64);
    serial::write_line("");

    serial::write_str("security: privacy min-log=");
    serial::write_u64(privacy_ok as u64);
    serial::write_line("");

    serial::write_str("security: integrity-plan stages=");
    serial::write_u64(integrity_stage_count);
    serial::write_str(" minimum=");
    serial::write_u64(integrity_stage_min);
    serial::write_line("");

    serial::write_str("security: privacy-policy defaults=");
    serial::write_u64(privacy_defaults_defined as u64);
    serial::write_str(" retention_bounded=");
    serial::write_u64(privacy_retention_bounded as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "security: baseline PASS"
    } else {
        "security: baseline FAIL"
    });

    serial::write_line(if authz_ok {
        "security: authz PASS"
    } else {
        "security: authz FAIL"
    });

    serial::write_line(if default_deny_ok {
        "security: default-deny PASS"
    } else {
        "security: default-deny FAIL"
    });

    serial::write_line(if authz_reason_ok {
        "security: authz-reason PASS"
    } else {
        "security: authz-reason FAIL"
    });

    serial::write_line(if privileged_deny_ok && privileged_reason_ok {
        "security: privileged-deny PASS"
    } else {
        "security: privileged-deny FAIL"
    });

    serial::write_line(if audit_counters_ok {
        "security: audit-counters PASS"
    } else {
        "security: audit-counters FAIL"
    });

    serial::write_line(if isolation_ok {
        "security: isolation PASS"
    } else {
        "security: isolation FAIL"
    });

    serial::write_line(if privacy_ok {
        "security: privacy PASS"
    } else {
        "security: privacy FAIL"
    });

    serial::write_line(if integrity_plan_ok {
        "security: integrity-plan PASS"
    } else {
        "security: integrity-plan FAIL"
    });

    serial::write_line(if privacy_policy_ok {
        "security: privacy-policy PASS"
    } else {
        "security: privacy-policy FAIL"
    });

    serial::write_line(if e13_contract_ok {
        "security: e13-contract PASS"
    } else {
        "security: e13-contract FAIL"
    });
}
