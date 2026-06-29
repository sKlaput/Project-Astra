use core::sync::atomic::{AtomicU64, Ordering};

use crate::{arch, idle, memory, scheduler, serial, syscall};

mod baselines;
mod heap_boot;
mod signals;

pub(crate) use baselines::{
    probe_e12_performance_baseline, probe_e13_security_baseline,
    probe_poste14_apic_transition_baseline, probe_poste14_packaging_signing_baseline,
};
pub(crate) use heap_boot::{probe_heap_mixed_stress, probe_heap_multi_page, probe_idle_for_ticks};
pub(crate) use signals::{
    probe_task_signal, probe_task_signal_blocking, probe_task_signal_telemetry,
    probe_task_signal_timeout,
};
