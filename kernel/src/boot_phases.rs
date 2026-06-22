/// Boot phase orchestration.
/// Organized by execution phase (E-series), moving from core infrastructure
/// to higher-level subsystem probing. Each phase is self-contained and called
/// only once from kmain.
use crate::{
    boot_probes, poste14_gui_probes, poste14_gui_probes_refactored, serial, subsystem_validation,
};

/// Phase E1–E2: Core memory and scheduler boot
pub fn phase_e1_e2_core() {
    serial::write_line("=== PHASE E1-E2: Core Kernel Boot ===");
    boot_probes::probe_heap_multi_page();
    boot_probes::probe_heap_mixed_stress();
}

/// Phase E2–E3: Timer, task, and synchronization primitives
pub fn phase_e2_e3_scheduler() {
    serial::write_line("=== PHASE E2-E3: Scheduler & Sync ===");
    boot_probes::probe_timer_interrupts();
    boot_probes::probe_scheduler_ticks();
    boot_probes::probe_scheduler_idle_decision();
    boot_probes::probe_scheduler_queue_api();

    // Task lifecycle and queue mechanics
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

    // Synchronization and signaling
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
    boot_probes::probe_preemption();
    boot_probes::probe_priority_aging();
    boot_probes::probe_aging_toggle();
    boot_probes::probe_aging_telemetry();
    boot_probes::probe_task_names();
    boot_probes::probe_priority_mutation();
    boot_probes::probe_priority_inheritance();
}

/// Phase E4–E9: System call interface and user-mode primitives
pub fn phase_e4_e9_syscall_user() {
    serial::write_line("=== PHASE E4-E9: Syscall & User Mode ===");
    boot_probes::probe_syscall_dispatch();
    boot_probes::probe_sleep_ticks();
    boot_probes::probe_idle_for_ticks();
    boot_probes::probe_ring3_descriptors();
    boot_probes::probe_syscall_entry_msrs();
    boot_probes::probe_ring3_user_mapping();
    boot_probes::probe_ring3_breakpoint_roundtrip();
    boot_probes::probe_syscall_sysret_roundtrip();
    boot_probes::probe_syscall_sysret_stack_stress();
    boot_probes::probe_syscall_abi_smoke_user();
    boot_probes::probe_syscall_abi_task_context();
    boot_probes::probe_persistent_user_task();
    boot_probes::probe_user_fault_isolation();
    boot_probes::probe_elf_loader();
}

/// Phase E12–E13: Performance and Security baselines
pub fn phase_e12_e13_baseline() {
    serial::write_line("=== PHASE E12-E13: Baselines ===");
    boot_probes::probe_e12_performance_baseline();
    boot_probes::probe_e13_security_baseline();
}

/// Phase E14 and post-E14: GUI, applications, and subsystem validation
pub fn phase_e14_poste14_gui_apps() {
    serial::write_line("=== PHASE E14+: GUI & Apps ===");

    // Core GUI infrastructure
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

    // Window manager and resource models
    boot_probes::probe_gui_window_manager();
    boot_probes::probe_process_model();
    boot_probes::probe_driver_model();

    // Network is still scaffolding
    boot_probes::probe_network_scaffold_v0();

    // Post-E14 transition baselines
    boot_probes::probe_poste14_apic_transition_baseline();
    boot_probes::probe_poste14_storage_persistence_baseline();
    boot_probes::probe_poste14_packaging_signing_baseline();

    // Real subsystem validation (NOT just flag aggregation)
    poste14_gui_probes::probe_subsystem_state_refactored();

    // VFS and default applications
    poste14_gui_probes::probe_poste14_gui_runtime_ownership_baseline();
    boot_probes::probe_vfs();
    boot_probes::probe_app_terminal_v0();
    boot_probes::probe_app_text_editor_v0();
    boot_probes::probe_app_file_manager_v0();
    boot_probes::probe_app_settings_v0();

    // Application lifecycle and lifecycle-adjacent GUI behavior
    poste14_gui_probes::probe_poste14_gui_app_lifecycle_baseline();
    poste14_gui_probes::probe_poste14_gui_runtime_composition_baseline();

    // Focus and input routing (core GUI responsibilities)
    poste14_gui_probes::probe_poste14_gui_focus_arbitration_baseline();
    poste14_gui_probes::probe_poste14_gui_input_routing_baseline();
    poste14_gui_probes::probe_poste14_gui_focus_recovery_baseline();

    // Event ordering and recovery (documented in post-E14 spec)
    poste14_gui_probes::probe_poste14_gui_event_ordering_baseline();
    poste14_gui_probes::probe_poste14_gui_recovery_escalation_baseline();

    // Extended GUI probe chain (cycles 2-5 with pattern variations)
    // This is where we run the multi-cycle health triplet variants
    run_poste14_gui_probe_chain();
}

/// Run post-E14 GUI probe chain (cycles 2-5).
/// Extracted from main.rs to keep phase orchestration clean.
///
/// CONSOLIDATED: V1 baseline probes (transition_churn, escalation_cooldown, etc.) have been
/// consolidated into a single baseline check. The original 20+ baseline probes were repeating
/// the same health-triplet pattern with different label combinations. Now we do one consolidated
/// baseline sweep, then move directly to meaningful extended probes (4-element permutations).
fn run_poste14_gui_probe_chain() {
    // REFACTORED: Consolidated baseline validates core preconditions
    probe_poste14_gui_consolidated_baseline();

    // REFACTORED: 24 permutation probes now use parameterized factory instead of
    // 24 individually-named functions. Replaces ~4000+ lines of boilerplate with
    // a single loop over a config table. Same functionality, no repetition.
    poste14_gui_probes_refactored::run_poste14_gui_permutations_refactored();

    // Cycles 3-5: Extended probe variations from original cycle modules
    poste14_gui_probes::run_poste14_gui_probe_chain();
}

/// Consolidated baseline V1 GUI validation.
/// Replaces 20+ individual baseline probes that were repeating the same health-triplet pattern.
/// This single probe validates essential GUI preconditions without the sprawl.
fn probe_poste14_gui_consolidated_baseline() {
    let uptime_before = crate::arch::x86_64::interrupts::uptime_ms();

    // Validate core GUI runtime preconditions
    let scheduler_ok = subsystem_validation::validate_scheduler_operational();
    let process_ok = subsystem_validation::validate_process_subsystem_present();
    let syscall_ok = subsystem_validation::validate_syscall_dispatch_safe();
    let context_switch_ok = subsystem_validation::validate_scheduler_context_switching();

    let uptime_after = crate::arch::x86_64::interrupts::uptime_ms();
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
