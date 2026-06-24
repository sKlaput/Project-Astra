mod boot_core;
mod drivers;
mod filesystem;
mod gui_framebuffer;
mod heap;
mod lapic_timer;
mod networking;
mod process_loader;
mod scheduler_advanced;
mod scheduler_basic;
mod sync_primitives;
mod syscall_ring3;

pub(crate) use boot_core::{
    probe_e12_performance_baseline, probe_e13_security_baseline, probe_heap_mixed_stress,
    probe_heap_multi_page, probe_idle_for_ticks, probe_poste14_apic_transition_baseline,
    probe_poste14_packaging_signing_baseline, probe_task_signal, probe_task_signal_blocking,
    probe_task_signal_telemetry, probe_task_signal_timeout,
};
pub(crate) use drivers::probe_driver_model;
pub(crate) use filesystem::{probe_poste14_storage_persistence_baseline, probe_vfs};
pub(crate) use gui_framebuffer::{
    probe_app_file_manager_v0, probe_app_settings_v0, probe_app_terminal_v0,
    probe_app_text_editor_v0, probe_gui_demo, probe_gui_fb_mapping, probe_gui_fb_mapping_user_task,
    probe_gui_window_manager, GUI_FB_DEEP_PROBE, GUI_FB_USER_DEEP_PROBE,
};
pub(crate) use heap::{
    heap_debug_ladder, probe_alloc_failure_path, HEAP_ALLOC_FAILURE_PROBE, HEAP_DEBUG,
};
pub(crate) use lapic_timer::probe_lapic_timer_switch;
pub(crate) use networking::probe_network_scaffold_v0;
pub(crate) use process_loader::probe_process_model;
pub(crate) use scheduler_advanced::{
    probe_aging_telemetry, probe_aging_toggle, probe_preemption, probe_priority_aging,
    probe_priority_inheritance, probe_priority_mutation, probe_scheduler_invariants,
    probe_scheduler_ring_overflow, probe_scheduler_stats, probe_scheduler_stats_guard,
    probe_scheduler_task_state, probe_task_dispatch, probe_task_lifecycle,
    probe_task_mixed_fairness, probe_task_names, probe_task_sleep_queue,
    probe_task_stress_sleep_mix, probe_task_wake_order,
};
pub(crate) use scheduler_basic::{
    probe_priority_order, probe_priority_slices, probe_scheduler_idle_decision,
    probe_scheduler_queue_api, probe_scheduler_ticks, probe_sleep_ticks, probe_timer_interrupts,
};
pub(crate) use sync_primitives::{
    probe_channel, probe_channel_stress, probe_channel_timeout, probe_condvar_notify_all,
    probe_condvar_notify_one, probe_condvar_timeout, probe_mutex_contention, probe_mutex_timeout,
    probe_park_unpark_telemetry, probe_rwlock, probe_rwlock_timeout, probe_semaphore,
    probe_semaphore_timeout, probe_spinlock, probe_sync_mix, probe_telemetry_monotone,
};
pub(crate) use syscall_ring3::{
    probe_elf_loader, probe_persistent_user_task, probe_ring3_breakpoint_roundtrip,
    probe_ring3_descriptors, probe_ring3_user_mapping, probe_syscall_abi_smoke_user,
    probe_syscall_abi_task_context, probe_syscall_dispatch, probe_syscall_entry_msrs,
    probe_syscall_sysret_roundtrip, probe_syscall_sysret_stack_stress, probe_user_fault_isolation,
};
