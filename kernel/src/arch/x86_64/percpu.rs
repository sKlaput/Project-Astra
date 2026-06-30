//! Per-CPU local storage via GSBASE segment register

use alloc::boxed::Box;

/// Per-core data structure size
pub const PERCPU_SIZE: usize = 4096;

#[repr(C, align(4096))]
pub struct PerCpuData {
    pub self_ptr: *const PerCpuData,
    pub cpu_id: u32,
    pub lapic_id: u32,
    pub current_task: usize,
    pub errno: u32,
    pub _pad1: u32,
    pub interrupt_count: u64,
    pub in_interrupt: u8,
    pub _padding: [u8; PERCPU_SIZE - 41],
}

unsafe impl Send for PerCpuData {}
unsafe impl Sync for PerCpuData {}

impl PerCpuData {
    pub fn new(lapic_id: u32) -> &'static mut Self {
        let mut data = Box::new(PerCpuData {
            self_ptr: core::ptr::null(),
            cpu_id: lapic_id,
            lapic_id,
            current_task: 0,
            errno: 0,
            _pad1: 0,
            interrupt_count: 0,
            in_interrupt: 0,
            _padding: [0; PERCPU_SIZE - 41],
        });
        
        let ptr = &mut *data as *mut PerCpuData;
        data.self_ptr = ptr as *const PerCpuData;
        
        Box::leak(data)
    }
}

/// Get the current CPU's per-core data structure
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn this_cpu() -> &'static mut PerCpuData {
    let ptr: *mut PerCpuData;
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[0]",
            out(reg) ptr,
            options(nostack, readonly)
        );
        &mut *ptr
    }
}

/// Get the current CPU's ID (LAPIC ID)
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn cpu_id() -> u32 {
    unsafe { this_cpu().cpu_id }
}

/// Get the current CPU's LAPIC ID
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn lapic_id() -> u32 {
    unsafe { this_cpu().lapic_id }
}

/// Get the currently executing task pointer
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn current_task() -> usize {
    unsafe { this_cpu().current_task }
}

/// Set the currently executing task pointer
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn set_current_task(task: usize) {
    unsafe { this_cpu().current_task = task }
}

/// Get the thread-local errno for this CPU
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn errno() -> u32 {
    unsafe { this_cpu().errno }
}

/// Set the thread-local errno for this CPU
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn set_errno(err: u32) {
    unsafe { this_cpu().errno = err }
}

/// Increment the interrupt counter for this CPU
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn inc_interrupt_count() {
    unsafe { this_cpu().interrupt_count = this_cpu().interrupt_count.wrapping_add(1) }
}

/// Get the interrupt counter for this CPU
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn interrupt_count() -> u64 {
    unsafe { this_cpu().interrupt_count }
}

/// Mark that we're entering an interrupt handler
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn enter_interrupt() {
    unsafe { this_cpu().in_interrupt += 1 }
}

/// Mark that we're exiting an interrupt handler
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn exit_interrupt() {
    unsafe { this_cpu().in_interrupt = this_cpu().in_interrupt.saturating_sub(1) }
}

/// Check if we're currently in an interrupt handler
/// # Safety: Must only be called after GSBASE has been set
#[inline(always)]
pub unsafe fn in_interrupt() -> bool {
    unsafe { this_cpu().in_interrupt > 0 }
}