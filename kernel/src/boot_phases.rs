/// Boot phase orchestration.
/// Organized by execution phase (E-series), moving from core infrastructure
/// to higher-level subsystem probing. Each phase is self-contained and called
/// only once from kmain.

use crate::*;

/// Phase E1–E2: Core memory and scheduler boot
pub fn phase_e1_e2_core() {
    serial::write_line("=== PHASE E1-E2: Core Kernel Boot ===");
    probe_heap_multi_page();
    probe_heap_mixed_stress();
}

/// Phase E2–E3: Timer, task, and synchronization primitives
pub fn phase_e2_e3_scheduler() {
    serial::write_line("=== PHASE E2-E3: Scheduler & Sync ===");
    probe_timer_interrupts();
    probe_scheduler_ticks();
    probe_scheduler_idle_decision();
    probe_scheduler_queue_api();
    
    // Task lifecycle and queue mechanics
    probe_scheduler_ring_overflow();
    probe_scheduler_task_state();
    probe_task_lifecycle();
    probe_task_dispatch();
    probe_task_sleep_queue();
    probe_task_wake_order();
    probe_task_mixed_fairness();
    probe_scheduler_invariants();
    probe_task_stress_sleep_mix();
    probe_scheduler_stats();
    probe_scheduler_stats_guard();
    
    // Synchronization and signaling
    probe_spinlock();
    probe_task_signal();
    probe_task_signal_timeout();
    probe_task_signal_blocking();
    probe_task_signal_telemetry();
    probe_mutex_contention();
    probe_priority_order();
    probe_priority_slices();
    probe_semaphore();
    probe_channel();
    probe_channel_stress();
    probe_channel_timeout();
    probe_semaphore_timeout();
    probe_mutex_timeout();
    probe_telemetry_monotone();
    probe_condvar_notify_one();
    probe_condvar_notify_all();
    probe_condvar_timeout();
    probe_sync_mix();
    probe_park_unpark_telemetry();
    probe_rwlock_timeout();
    probe_rwlock();
    probe_preemption();
    probe_priority_aging();
    probe_aging_toggle();
    probe_aging_telemetry();
    probe_task_names();
    probe_priority_mutation();
    probe_priority_inheritance();
}

/// Phase E4–E9: System call interface and user-mode primitives
pub fn phase_e4_e9_syscall_user() {
    serial::write_line("=== PHASE E4-E9: Syscall & User Mode ===");
    probe_syscall_dispatch();
    probe_sleep_ticks();
    probe_idle_for_ticks();
    probe_ring3_descriptors();
    probe_syscall_entry_msrs();
    probe_ring3_user_mapping();
    probe_ring3_breakpoint_roundtrip();
    probe_syscall_sysret_roundtrip();
    probe_syscall_sysret_stack_stress();
    probe_syscall_abi_smoke_user();
    probe_syscall_abi_task_context();
    probe_persistent_user_task();
    probe_user_fault_isolation();
    probe_elf_loader();
}

/// Phase E12–E13: Performance and Security baselines
pub fn phase_e12_e13_baseline() {
    serial::write_line("=== PHASE E12-E13: Baselines ===");
    probe_e12_performance_baseline();
    probe_e13_security_baseline();
}

/// Phase E14 and post-E14: GUI, applications, and subsystem validation
pub fn phase_e14_poste14_gui_apps() {
    serial::write_line("=== PHASE E14+: GUI & Apps ===");
    
    // Core GUI infrastructure
    probe_gui_demo();
    serial::write_str("gui: diag kernel_deep=");
    serial::write_u64(super::GUI_FB_DEEP_PROBE as u64);
    serial::write_str(" user_deep=");
    serial::write_u64(super::GUI_FB_USER_DEEP_PROBE as u64);
    serial::write_line("");
    
    probe_gui_fb_mapping();
    if super::GUI_FB_USER_DEEP_PROBE {
        probe_gui_fb_mapping_user_task();
    } else {
        serial::write_line("gui: fb-map-user SKIP (gated)");
    }
    
    // Window manager and resource models
    probe_gui_window_manager();
    probe_process_model();
    probe_driver_model();
    
    // Network is still scaffolding
    probe_network_scaffold_v0();
    
    // Post-E14 transition baselines
    probe_poste14_apic_transition_baseline();
    probe_poste14_storage_persistence_baseline();
    probe_poste14_packaging_signing_baseline();
    
    // Real subsystem validation (NOT just flag aggregation)
    poste14_gui_probes::probe_subsystem_state_refactored();
    
    // VFS and default applications
    probe_poste14_gui_runtime_ownership_baseline();
    probe_vfs();
    probe_app_terminal_v0();
    probe_app_text_editor_v0();
    probe_app_file_manager_v0();
    probe_app_settings_v0();
    
    // Application lifecycle and lifecycle-adjacent GUI behavior
    probe_poste14_gui_app_lifecycle_baseline();
    probe_poste14_gui_runtime_composition_baseline();
    
    // Focus and input routing (core GUI responsibilities)
    probe_poste14_gui_focus_arbitration_baseline();
    probe_poste14_gui_input_routing_baseline();
    probe_poste14_gui_focus_recovery_baseline();
    
    // Event ordering and recovery (documented in post-E14 spec)
    probe_poste14_gui_event_ordering_baseline();
    probe_poste14_gui_recovery_escalation_baseline();
    
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
    crate::poste14_gui_probes_refactored::run_poste14_gui_permutations_refactored();
    
    // Cycles 3-5: Extended probe variations from original cycle modules
    poste14_gui_probes::run_poste14_gui_probe_chain();
}

/// Consolidated baseline V1 GUI validation.
/// Replaces 20+ individual baseline probes that were repeating the same health-triplet pattern.
/// This single probe validates essential GUI preconditions without the sprawl.
fn probe_poste14_gui_consolidated_baseline() {
    use crate::subsystem_validation;
    use crate::serial;
    
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
