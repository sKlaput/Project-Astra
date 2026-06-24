/// Boot phase orchestration.
/// Organized by execution phase (E-series), moving from core infrastructure
/// to higher-level subsystem probing. Each phase is self-contained and called
/// only once from kmain.
use crate::{
    arch, boot_probes, poste14_gui_probes, poste14_gui_probes_refactored, serial,
    subsystem_validation,
};

pub const RUN_SYSCALL_USER_PHASE: bool = false;
pub const RUN_E12_E13_BASELINE_PHASE: bool = false;
pub const RUN_E14_GUI_APPS_PHASE: bool = false;

/// Phase E1-E2: Core memory and scheduler boot.
pub fn phase_e1_e2_core() {
    serial::write_line("=== PHASE E1-E2: Core Kernel Boot ===");
    boot_probes::probe_heap_multi_page();
    boot_probes::probe_heap_mixed_stress();
}

/// Phase E2-E3: Timer, task, and synchronization primitives.
pub fn phase_e2_e3_scheduler() {
    serial::write_line("=== PHASE E2-E3: Scheduler & Sync ===");
    run_timer_and_scheduler_basics();
    run_scheduler_task_model_probes();
    run_sync_and_signal_probes();
    run_scheduler_policy_probes();
}

/// Keep heavier or currently unsafe phases visible without enabling them by
/// accident. The default boot path remains the same: these phases stay off.
pub fn run_deferred_optional_phases() {
    run_optional_phase(
        "E4-E9",
        RUN_SYSCALL_USER_PHASE,
        "syscall/ring3 probes disabled",
        phase_e4_e9_syscall_user,
    );
    run_optional_phase(
        "E12-E13",
        RUN_E12_E13_BASELINE_PHASE,
        "baseline probes disabled",
        phase_e12_e13_baseline,
    );
    run_optional_phase(
        "E14+",
        RUN_E14_GUI_APPS_PHASE,
        "GUI/app probes disabled",
        phase_e14_poste14_gui_apps,
    );
}

/// Phase E4-E9: System call interface and user-mode primitives.
pub fn phase_e4_e9_syscall_user() {
    serial::write_line("=== PHASE E4-E9: Syscall & User Mode ===");
    run_syscall_dispatch_probes();
    run_ring3_descriptor_probes();
    run_ring3_execution_probes();
}

/// Phase E12-E13: Performance and Security baselines.
pub fn phase_e12_e13_baseline() {
    serial::write_line("=== PHASE E12-E13: Baselines ===");
    boot_probes::probe_e12_performance_baseline();
    boot_probes::probe_e13_security_baseline();
}

/// Phase E14 and post-E14: GUI, applications, and subsystem validation.
pub fn phase_e14_poste14_gui_apps() {
    serial::write_line("=== PHASE E14+: GUI & Apps ===");
    run_gui_infrastructure_probes();
    run_gui_resource_model_probes();
    run_poste14_transition_baselines();
    run_gui_subsystem_validation_probes();
    run_default_app_probes();
    run_gui_lifecycle_probes();
    run_gui_focus_and_event_probes();
    run_poste14_gui_probe_chain();
}

fn run_optional_phase(name: &str, enabled: bool, skip_reason: &str, phase: fn()) {
    if enabled {
        phase();
    } else {
        serial::write_str("boot: phase ");
        serial::write_str(name);
        serial::write_str(" SKIP (gated: ");
        serial::write_str(skip_reason);
        serial::write_line(")");
    }
}

fn run_timer_and_scheduler_basics() {
    boot_probes::probe_timer_interrupts();
    boot_probes::probe_scheduler_ticks();
    boot_probes::probe_scheduler_idle_decision();
    boot_probes::probe_scheduler_queue_api();
}

fn run_scheduler_task_model_probes() {
    boot_probes::probe_scheduler_ring_overflow();
    boot_probes::probe_scheduler_task_state();
    boot_probes::probe_task_lifecycle();
    boot_probes::probe_task_dispatch();
    boot_probes::probe_task_sleep_queue();
    boot_probes::probe_task_wake_order();
    boot_probes::probe_task_mixed_fairness();
    boot_probes::probe_scheduler_invariants();
    boot_probes::probe_task_stress_sleep_mix();
    boot_probes::probe_scheduler_stats();
    boot_probes::probe_scheduler_stats_guard();
}

fn run_sync_and_signal_probes() {
    boot_probes::probe_spinlock();
    boot_probes::probe_task_signal();
    boot_probes::probe_task_signal_timeout();
    boot_probes::probe_task_signal_blocking();
    boot_probes::probe_task_signal_telemetry();
    boot_probes::probe_mutex_contention();
    boot_probes::probe_priority_order();
    boot_probes::probe_priority_slices();
    boot_probes::probe_semaphore();
    boot_probes::probe_channel();
    boot_probes::probe_channel_stress();
    boot_probes::probe_channel_timeout();
    boot_probes::probe_semaphore_timeout();
    boot_probes::probe_mutex_timeout();
    boot_probes::probe_telemetry_monotone();
    boot_probes::probe_condvar_notify_one();
    boot_probes::probe_condvar_notify_all();
    boot_probes::probe_condvar_timeout();
    boot_probes::probe_sync_mix();
    boot_probes::probe_park_unpark_telemetry();
    boot_probes::probe_rwlock_timeout();
    boot_probes::probe_rwlock();
}

fn run_scheduler_policy_probes() {
    boot_probes::probe_preemption();
    boot_probes::probe_priority_aging();
    boot_probes::probe_aging_toggle();
    boot_probes::probe_aging_telemetry();
    boot_probes::probe_task_names();
    boot_probes::probe_priority_mutation();
    boot_probes::probe_priority_inheritance();
}

fn run_syscall_dispatch_probes() {
    boot_probes::probe_syscall_dispatch();
    boot_probes::probe_sleep_ticks();
    boot_probes::probe_idle_for_ticks();
}

fn run_ring3_descriptor_probes() {
    boot_probes::probe_ring3_descriptors();
    boot_probes::probe_syscall_entry_msrs();
    boot_probes::probe_ring3_user_mapping();
}

fn run_ring3_execution_probes() {
    boot_probes::probe_ring3_breakpoint_roundtrip();
    boot_probes::probe_syscall_sysret_roundtrip();
    boot_probes::probe_syscall_sysret_stack_stress();
    boot_probes::probe_syscall_abi_smoke_user();
    boot_probes::probe_syscall_abi_task_context();
    boot_probes::probe_persistent_user_task();
    boot_probes::probe_user_fault_isolation();
    boot_probes::probe_elf_loader();
}

fn run_gui_infrastructure_probes() {
    boot_probes::probe_gui_demo();
    serial::write_str("gui: diag kernel_deep=");
    serial::write_u64(boot_probes::GUI_FB_DEEP_PROBE as u64);
    serial::write_str(" user_deep=");
    serial::write_u64(boot_probes::GUI_FB_USER_DEEP_PROBE as u64);
    serial::write_line("");

    boot_probes::probe_gui_fb_mapping();
    if boot_probes::GUI_FB_USER_DEEP_PROBE {
        boot_probes::probe_gui_fb_mapping_user_task();
    } else {
        serial::write_line("gui: fb-map-user SKIP (gated)");
    }
}

fn run_gui_resource_model_probes() {
    boot_probes::probe_gui_window_manager();
    boot_probes::probe_process_model();
    boot_probes::probe_driver_model();
    boot_probes::probe_network_scaffold_v0();
}

fn run_poste14_transition_baselines() {
    boot_probes::probe_poste14_apic_transition_baseline();
    boot_probes::probe_poste14_storage_persistence_baseline();
    boot_probes::probe_poste14_packaging_signing_baseline();
}

fn run_gui_subsystem_validation_probes() {
    poste14_gui_probes::probe_subsystem_state_refactored();
    poste14_gui_probes::probe_poste14_gui_runtime_ownership_baseline();
}

fn run_default_app_probes() {
    boot_probes::probe_vfs();
    boot_probes::probe_app_terminal_v0();
    boot_probes::probe_app_text_editor_v0();
    boot_probes::probe_app_file_manager_v0();
    boot_probes::probe_app_settings_v0();
}

fn run_gui_lifecycle_probes() {
    poste14_gui_probes::probe_poste14_gui_app_lifecycle_baseline();
    poste14_gui_probes::probe_poste14_gui_runtime_composition_baseline();
}

fn run_gui_focus_and_event_probes() {
    poste14_gui_probes::probe_poste14_gui_focus_arbitration_baseline();
    poste14_gui_probes::probe_poste14_gui_input_routing_baseline();
    poste14_gui_probes::probe_poste14_gui_focus_recovery_baseline();
    poste14_gui_probes::probe_poste14_gui_event_ordering_baseline();
    poste14_gui_probes::probe_poste14_gui_recovery_escalation_baseline();
}

/// Run post-E14 GUI probe chain (cycles 2-5).
fn run_poste14_gui_probe_chain() {
    probe_poste14_gui_consolidated_baseline();
    poste14_gui_probes_refactored::run_poste14_gui_permutations_refactored();
    poste14_gui_probes::run_poste14_gui_probe_chain();
}

fn probe_poste14_gui_consolidated_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();

    let scheduler_ok = subsystem_validation::validate_scheduler_operational();
    let process_ok = subsystem_validation::validate_process_subsystem_present();
    let syscall_ok = subsystem_validation::validate_syscall_dispatch_safe();
    let context_switch_ok = subsystem_validation::validate_scheduler_context_switching();

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let uptime_delta = uptime_after.saturating_sub(uptime_before);

    let baseline_ok = scheduler_ok && process_ok && syscall_ok && context_switch_ok;

    serial::write_str("gui: consolidated-baseline scheduler=");
    serial::write_u64(scheduler_ok as u64);
    serial::write_str(" process=");
    serial::write_u64(process_ok as u64);
    serial::write_str(" syscall=");
    serial::write_u64(syscall_ok as u64);
    serial::write_str(" ctxsw=");
    serial::write_u64(context_switch_ok as u64);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_delta);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "gui: consolidated-baseline PASS"
    } else {
        "gui: consolidated-baseline FAIL"
    });
}
