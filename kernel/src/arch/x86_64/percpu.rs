//! Per-CPU local storage via GSBASE segment register
//! Phase 3: Per-core task queue for multicore scheduler

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Per-core data structure size (1 page, 4096 bytes)
pub const PERCPU_SIZE: usize = 4096;
pub const QUEUE_CAP: usize = 8;  // Tasks per core queue

#[repr(C, align(4096))]
pub struct PerCpuData {
    /// Offset 0: Points to self (enables gs:[0] access)
    pub self_ptr: *const PerCpuData,
    
    /// Offset 8: CPU identifier (LAPIC ID)
    pub cpu_id: u32,
    
    /// Offset 12: Duplicate of cpu_id
    pub lapic_id: u32,
    
    /// Offset 16: Currently executing task pointer
    pub current_task: usize,
    
    /// Offset 24: Thread-local errno
    pub errno: u32,
    
    /// Offset 28: Padding
    pub _pad1: u32,
    
    /// Offset 32: Count of interrupts on this CPU
    pub interrupt_count: u64,
    
    /// Offset 40: Currently in interrupt handler
    pub in_interrupt: u8,
    
    // ========== PHASE 3: Per-Core Queue State ==========
    /// Offset 41: Queue dequeue pointer (head)
    pub queue_head: AtomicU32,
    
    /// Offset 45: Queue enqueue pointer (tail)
    pub queue_tail: AtomicU32,
    
    /// Offset 49: Spinlock for queue (0=unlocked, 1=locked)
    pub queue_lock: AtomicU32,
    
    /// Offset 53: Padding to align queue_buf
    pub _pad2: [u8; 3],
    
    /// Offset 56: Task IDs in queue (8 slots × 8 bytes = 64 bytes)
    pub queue_buf: [AtomicU64; QUEUE_CAP],
    
    // ========== Remaining space for future expansion ==========
    /// Offset 120-4095: Reserved (3976 bytes available)
    pub _padding: [u8; PERCPU_SIZE - 120],
}

unsafe impl Send for PerCpuData {}
unsafe impl Sync for PerCpuData {}

// Verify struct layout matches offset assumptions
const _: () = {
    const fn check_offsets() {
        let _ = core::mem::offset_of!(PerCpuData, self_ptr);      // 0
        let _ = core::mem::offset_of!(PerCpuData, cpu_id);        // 8
        let _ = core::mem::offset_of!(PerCpuData, lapic_id);      // 12
        let _ = core::mem::offset_of!(PerCpuData, current_task);  // 16
        let _ = core::mem::offset_of!(PerCpuData, errno);         // 24
        let _ = core::mem::offset_of!(PerCpuData, _pad1);         // 28
        let _ = core::mem::offset_of!(PerCpuData, interrupt_count); // 32
        let _ = core::mem::offset_of!(PerCpuData, in_interrupt);  // 40
        let _ = core::mem::offset_of!(PerCpuData, queue_head);    // 41
        let _ = core::mem::offset_of!(PerCpuData, queue_tail);    // 45
        let _ = core::mem::offset_of!(PerCpuData, queue_lock);    // 49
        let _ = core::mem::offset_of!(PerCpuData, queue_buf);     // 56
    }
};

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
            // Phase 3: Initialize queue state
            queue_head: AtomicU32::new(0),
            queue_tail: AtomicU32::new(0),
            queue_lock: AtomicU32::new(0),
            _pad2: [0; 3],
            queue_buf: [
                AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0),
            ],
            _padding: [0; PERCPU_SIZE - 120],
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
    }
    unsafe { &mut *ptr }
}

/// Get the current CPU's ID (LAPIC ID)
#[inline(always)]
pub unsafe fn cpu_id() -> u32 {
    unsafe { this_cpu().cpu_id }
}

/// Get the current CPU's LAPIC ID
#[inline(always)]
pub unsafe fn lapic_id() -> u32 {
    unsafe { this_cpu().lapic_id }
}

/// Get the currently executing task pointer
#[inline(always)]
pub unsafe fn current_task() -> usize {
    unsafe { this_cpu().current_task }
}

/// Set the currently executing task pointer
#[inline(always)]
pub unsafe fn set_current_task(task: usize) {
    unsafe { this_cpu().current_task = task }
}

/// Get the thread-local errno
#[inline(always)]
pub unsafe fn errno() -> u32 {
    unsafe { this_cpu().errno }
}

/// Set the thread-local errno
#[inline(always)]
pub unsafe fn set_errno(err: u32) {
    unsafe { this_cpu().errno = err }
}

/// Increment the interrupt counter
#[inline(always)]
pub unsafe fn inc_interrupt_count() {
    unsafe { this_cpu().interrupt_count = this_cpu().interrupt_count.wrapping_add(1) }
}

/// Get the interrupt counter
#[inline(always)]
pub unsafe fn interrupt_count() -> u64 {
    unsafe { this_cpu().interrupt_count }
}

/// Mark entering an interrupt handler
#[inline(always)]
pub unsafe fn enter_interrupt() {
    unsafe { this_cpu().in_interrupt += 1 }
}

/// Mark exiting an interrupt handler
#[inline(always)]
pub unsafe fn exit_interrupt() {
    unsafe { this_cpu().in_interrupt = this_cpu().in_interrupt.saturating_sub(1) }
}

/// Check if currently in interrupt handler
#[inline(always)]
pub unsafe fn in_interrupt() -> bool {
    unsafe { this_cpu().in_interrupt > 0 }
}

// ========== PHASE 3: Per-Core Queue Accessors ==========

/// Try to acquire the queue lock for this CPU (non-blocking)
#[inline(always)]
pub unsafe fn queue_try_lock() -> bool {
    unsafe {
        this_cpu().queue_lock.compare_exchange_weak(
            0, 1, Ordering::Acquire, Ordering::Relaxed
        ).is_ok()
    }
}

/// Release the queue lock for this CPU
#[inline(always)]
pub unsafe fn queue_unlock() {
    unsafe { this_cpu().queue_lock.store(0, Ordering::Release) }
}

/// Get queue head pointer (dequeue position)
#[inline(always)]
pub unsafe fn queue_head() -> u32 {
    unsafe { this_cpu().queue_head.load(Ordering::Relaxed) }
}

/// Get queue tail pointer (enqueue position)
#[inline(always)]
pub unsafe fn queue_tail() -> u32 {
    unsafe { this_cpu().queue_tail.load(Ordering::Relaxed) }
}

/// Set queue head pointer
#[inline(always)]
pub unsafe fn set_queue_head(head: u32) {
    unsafe { this_cpu().queue_head.store(head, Ordering::Relaxed) }
}

/// Set queue tail pointer
#[inline(always)]
pub unsafe fn set_queue_tail(tail: u32) {
    unsafe { this_cpu().queue_tail.store(tail, Ordering::Release) }
}

/// Get task ID at queue position
#[inline(always)]
pub unsafe fn queue_get(index: usize) -> u64 {
    unsafe { this_cpu().queue_buf[index % QUEUE_CAP].load(Ordering::Relaxed) }
}

/// Set task ID at queue position
#[inline(always)]
pub unsafe fn queue_set(index: usize, task_id: u64) {
    unsafe { this_cpu().queue_buf[index % QUEUE_CAP].store(task_id, Ordering::Relaxed) }
}

/// Get queue capacity
pub const fn queue_capacity() -> usize {
    QUEUE_CAP
}

// ========== PHASE 3.4: Work-Stealing Support ==========

/// Global array to store pointers to all CPUs' PerCpuData
/// Maximum 256 CPUs supported
use core::sync::atomic::AtomicPtr;
static PER_CORE_DATA: [AtomicPtr<PerCpuData>; 256] = {
    const INIT: AtomicPtr<PerCpuData> = AtomicPtr::new(core::ptr::null_mut());
    [INIT; 256]
};

/// Register a CPU's PerCpuData for work-stealing access
/// # Safety: Must be called exactly once per CPU during initialization
#[inline]
pub unsafe fn register_percpu_data(cpu_id: u32, percpu_ptr: *mut PerCpuData) {
    if cpu_id < 256 {
        PER_CORE_DATA[cpu_id as usize].store(percpu_ptr, Ordering::Release);
    }
}

/// Get another CPU's PerCpuData for work-stealing
/// # Safety: The returned reference may be invalidated if the target CPU is uninitialized
#[inline]
pub unsafe fn get_percpu_data(cpu_id: u32) -> Option<&'static mut PerCpuData> {
    if cpu_id < 256 {
        let ptr = PER_CORE_DATA[cpu_id as usize].load(Ordering::Acquire);
        if !ptr.is_null() {
        return Some(unsafe { &mut *ptr });
        }
    }
    None
}
