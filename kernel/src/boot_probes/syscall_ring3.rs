use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::registers::rflags::RFlags;

use crate::{arch, idle, loader, memory, scheduler, serial, syscall};

mod dispatch;
mod ring3_core;
mod user_process;

pub(crate) use dispatch::probe_syscall_dispatch;
pub(crate) use ring3_core::{
    probe_ring3_breakpoint_roundtrip, probe_ring3_descriptors, probe_ring3_user_mapping,
    probe_syscall_abi_smoke_user, probe_syscall_abi_task_context, probe_syscall_entry_msrs,
    probe_syscall_sysret_roundtrip, probe_syscall_sysret_stack_stress,
};
pub(crate) use user_process::{
    probe_elf_loader, probe_persistent_user_task, probe_user_fault_isolation,
};
