//! Kernel task scheduler — cooperative + preemptive multi-tasking.
//!
//! Refactored architecture:
//! - table.rs: Consolidated task metadata
//! - context.rs: Low-level context switching & stack allocation
//! - dispatch.rs: Ready queue management & dequeuing
//! - signal.rs: Signal operations
//! - sleep.rs: Sleep/wake/park operations
//! - stats.rs: Telemetry collection

pub mod context;
pub mod dispatch;
pub mod signal;
pub mod sleep;
pub mod stats;
pub mod table;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::scheduler::table::{TASK_TABLE, TABLE_CAP, table_slot};

// Re-export key types from table
pub use table::{TaskId, TaskState};
pub use dispatch::RING_CAP;

/// Get the ring buffer capacity.
pub fn ring_capacity() -> usize {
    RING_CAP
}

// ============================================================================
// Global Scheduler State
// ============================================================================

pub static CURRENT_TASK: AtomicU64 = AtomicU64::new(0);
pub static SCHEDULER_CONTEXT_RSP: AtomicU64 = AtomicU64::new(0);
pub static IN_TASK_DISPATCH: AtomicBool = AtomicBool::new(false);
pub static IDLE_DECISION_SEEN: AtomicBool = AtomicBool::new(false);
pub static IDLE_DECISION_PENDING: AtomicBool = AtomicBool::new(false);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);
static SCHED_TICKS: AtomicU64 = AtomicU64::new(0);
static KERNEL_PML4_PHYS: AtomicU64 = AtomicU64::new(0);

// ============================================================================
// Utilities
// ============================================================================

fn interrupts_were_enabled() -> bool {
    let rflags: usize;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) rflags, options(nomem, preserves_flags));
    }
    (rflags & (1 << 9)) != 0
}

fn with_interrupts_masked<T>(f: impl FnOnce() -> T) -> T {
    let were_enabled = interrupts_were_enabled();
    if were_enabled {
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        }
    }
    let result = f();
    if were_enabled {
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
        }
    }
    result
}

fn kernel_pml4_phys() -> usize {
    let cached = KERNEL_PML4_PHYS.load(Ordering::Acquire);
    if cached != 0 {
        return cached as usize;
    }
    let cur = crate::memory::paging::current_cr3_phys();
    let _ = KERNEL_PML4_PHYS.compare_exchange(0, cur as u64, Ordering::AcqRel, Ordering::Acquire);
    KERNEL_PML4_PHYS.load(Ordering::Acquire) as usize
}

fn set_task_state(id: TaskId, state: TaskState) {
    let slot = table_slot(id.0);
    TASK_TABLE[slot].id.store(id.0, Ordering::Release);
    TASK_TABLE[slot].state.store(state as u8, Ordering::Release);
}

fn clear_task_state(id: TaskId) {
    let slot = table_slot(id.0);
    let _ = TASK_TABLE[slot].state.compare_exchange(
        TaskState::Ready as u8,
        TaskState::Empty as u8,
        Ordering::Release,
        Ordering::Relaxed,
    );
    let _ = TASK_TABLE[slot].state.compare_exchange(
        TaskState::Running as u8,
        TaskState::Empty as u8,
        Ordering::Release,
        Ordering::Relaxed,
    );
    let _ = TASK_TABLE[slot].state.compare_exchange(
        TaskState::Sleeping as u8,
        TaskState::Empty as u8,
        Ordering::Release,
        Ordering::Relaxed,
    );
    let _ = TASK_TABLE[slot].id.compare_exchange(id.0, 0, Ordering::Relaxed, Ordering::Relaxed);
    TASK_TABLE[slot].fn_ptr.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].wake_tick.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].enqueue_tick.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].reset_priority();
    TASK_TABLE[slot].preempted.store(false, Ordering::Relaxed);
    TASK_TABLE[slot].name_ptr.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].name_len.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].signals.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].signal_mask.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].context_rsp.store(0, Ordering::Relaxed);
    
    let stack_base = TASK_TABLE[slot].stack_base.swap(0, Ordering::Relaxed);
    context::dealloc_task_stack(stack_base);
    
    TASK_TABLE[slot].user_code_virt.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].user_stack_virt.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].user_entry_rip.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].user_rsp.store(0, Ordering::Relaxed);
    TASK_TABLE[slot].user_pml4.store(0, Ordering::Relaxed);
}

fn user_task_trampoline() {
    loop {
        let id = match current_task() {
            Some(id) => id,
            None => loop {
                unsafe {
                    core::arch::asm!("hlt", options(nomem, nostack));
                }
            },
        };

        let Some((entry_rip, user_rsp)) = get_task_user_entry(id) else {
            exit_task(id);
            continue;
        };

        let Some(user_pml4) = get_task_user_pml4(id) else {
            exit_task(id);
            continue;
        };

        unsafe { crate::memory::paging::switch_cr3(user_pml4 as usize) };

        let user_cs = crate::arch::x86_64::gdt::ring3_code_selector().0 as u64;
        let user_ss = crate::arch::x86_64::gdt::ring3_data_selector().0 as u64;
        let mut rflags: u64;
        unsafe {
            core::arch::asm!("pushfq", "pop {}", out(reg) rflags, options(nomem, preserves_flags));
        }
        let user_rflags = rflags | (1 << 9);

        crate::arch::x86_64::ring3::clear_saved_resume_rsp();
        unsafe {
            crate::arch::x86_64::ring3::enter_user_mode(
                entry_rip,
                user_rsp,
                user_cs,
                user_ss,
                user_rflags,
            );
        }
        unsafe { crate::memory::paging::switch_cr3(kernel_pml4_phys()) };
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }

        let _ = sleep_current_for_ticks(1);
    }
}

// ============================================================================
// Public API - Task Management
// ============================================================================

pub fn current_task() -> Option<TaskId> {
    let raw = unsafe { crate::arch::x86_64::percpu::this_cpu().current_task as u64 };
    if raw == 0 {
        None
    } else {
        Some(TaskId(raw))
    }
}

pub fn task_state(id: TaskId) -> TaskState {
    let slot = table_slot(id.0);
    if TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        TaskState::from_u8(TASK_TABLE[slot].state.load(Ordering::Acquire))
    } else {
        TaskState::Empty
    }
}

pub fn task_name(id: TaskId) -> &'static str {
    let slot = table_slot(id.0);
    if TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
        return "";
    }
    let ptr = TASK_TABLE[slot].name_ptr.load(Ordering::Relaxed) as *const u8;
    let len = TASK_TABLE[slot].name_len.load(Ordering::Relaxed) as usize;
    if ptr.is_null() || len == 0 {
        return "";
    }
    unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len)) }
}

pub fn spawn_task() -> Option<TaskId> {
    let task_id = TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed));
    if dispatch::enqueue_task_inner(task_id) {
        let slot = table_slot(task_id.0);
        TASK_TABLE[slot].fn_ptr.store(0, Ordering::Relaxed);
        TASK_TABLE[slot].wake_tick.store(0, Ordering::Relaxed);
        set_task_state(task_id, TaskState::Ready);
        Some(task_id)
    } else {
        None
    }
}

pub fn exit_task(id: TaskId) {
    stats::record_exit();
    CURRENT_TASK.compare_exchange(id.0, 0, Ordering::AcqRel, Ordering::Acquire).ok();
    let slot = table_slot(id.0);
    let percpu = unsafe { crate::arch::x86_64::percpu::this_cpu() };
    percpu.current_task = 0;
    clear_task_state(id);

    if percpu.in_task_dispatch {
        let sched_rsp = percpu.scheduler_rsp;
        percpu.in_task_dispatch = false;
        IN_TASK_DISPATCH.store(false, Ordering::Release);
        unsafe {
            context::context_switch(TASK_TABLE[slot].context_rsp.as_ptr(), sched_rsp);
        }
        loop {
            unsafe {
                core::arch::asm!("hlt", options(nomem, nostack));
            }
        }
    }
}

pub fn abort_current_user_task_from_fault() -> ! {
    unsafe { crate::memory::paging::switch_cr3(kernel_pml4_phys()) };

    let percpu = unsafe { crate::arch::x86_64::percpu::this_cpu() };
    if let Some(id) = current_task() {
        stats::record_exit();
        CURRENT_TASK.compare_exchange(id.0, 0, Ordering::AcqRel, Ordering::Acquire).ok();
        percpu.current_task = 0;
        clear_task_state(id);
    }
    let sched_rsp = percpu.scheduler_rsp;
    percpu.in_task_dispatch = false;
    IN_TASK_DISPATCH.store(false, Ordering::Release);
    if sched_rsp != 0 {
        unsafe { context::context_restore_to(sched_rsp) }
    } else {
        loop {
            unsafe {
                core::arch::asm!("hlt", options(nomem, nostack));
            }
        }
    }
}

pub fn spawn_task_with_fn(f: fn()) -> Option<TaskId> {
    spawn_task_with_fn_prio_name(f, 128, "")
}

pub fn spawn_task_with_fn_prio(f: fn(), priority: u8) -> Option<TaskId> {
    spawn_task_with_fn_prio_name(f, priority, "")
}

pub fn spawn_task_with_fn_prio_name(f: fn(), priority: u8, name: &'static str) -> Option<TaskId> {
    let task_id = TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed));
    if dispatch::enqueue_task_inner(task_id) {
        let slot = table_slot(task_id.0);
        let (initial_rsp, stack_base) = context::alloc_task_context(f);
        TASK_TABLE[slot].fn_ptr.store(f as usize as u64, Ordering::Relaxed);
        TASK_TABLE[slot].context_rsp.store(initial_rsp, Ordering::Release);
        TASK_TABLE[slot].stack_base.store(stack_base, Ordering::Relaxed);
        TASK_TABLE[slot].wake_tick.store(0, Ordering::Relaxed);
        TASK_TABLE[slot].priority.store(priority, Ordering::Relaxed);
        TASK_TABLE[slot].slice.store(dispatch::slice_for_priority(priority), Ordering::Relaxed);
        TASK_TABLE[slot].name_ptr.store(name.as_ptr() as u64, Ordering::Relaxed);
        TASK_TABLE[slot].name_len.store(name.len() as u64, Ordering::Relaxed);
        set_task_state(task_id, TaskState::Ready);
        Some(task_id)
    } else {
        None
    }
}

pub fn spawn_user_task_prio_name(
    code_virt: u64,
    stack_virt: u64,
    entry_rip: u64,
    user_rsp: u64,
    priority: u8,
    name: &'static str,
) -> Option<TaskId> {
    let task_id = spawn_task_with_fn_prio_name(user_task_trampoline, priority, name)?;
    if set_task_user_mode(task_id, code_virt, stack_virt, entry_rip, user_rsp) {
        Some(task_id)
    } else {
        exit_task(task_id);
        None
    }
}

pub fn spawn_user_task_prio(
    code_virt: u64,
    stack_virt: u64,
    entry_rip: u64,
    user_rsp: u64,
    priority: u8,
) -> Option<TaskId> {
    spawn_user_task_prio_name(code_virt, stack_virt, entry_rip, user_rsp, priority, "")
}

pub fn spawn_user_task(
    code_virt: u64,
    stack_virt: u64,
    entry_rip: u64,
    user_rsp: u64,
) -> Option<TaskId> {
    spawn_user_task_prio_name(code_virt, stack_virt, entry_rip, user_rsp, 128, "")
}

// ============================================================================
// Public API - User Task Mode Management
// ============================================================================

pub fn set_task_user_mode(
    id: TaskId,
    code_virt: u64,
    stack_virt: u64,
    entry_rip: u64,
    user_rsp: u64,
) -> bool {
    let slot = table_slot(id.0);
    if TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
        return false;
    }
    TASK_TABLE[slot].user_code_virt.store(code_virt, Ordering::Relaxed);
    TASK_TABLE[slot].user_stack_virt.store(stack_virt, Ordering::Relaxed);
    TASK_TABLE[slot].user_entry_rip.store(entry_rip, Ordering::Relaxed);
    TASK_TABLE[slot].user_rsp.store(user_rsp, Ordering::Relaxed);
    TASK_TABLE[slot].user_pml4.store(
        crate::memory::paging::current_cr3_phys() as u64,
        Ordering::Relaxed,
    );
    true
}

pub fn set_task_user_pml4(id: TaskId, pml4_phys: u64) -> bool {
    let slot = table_slot(id.0);
    if TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
        return false;
    }
    TASK_TABLE[slot].user_pml4.store(pml4_phys, Ordering::Relaxed);
    true
}

pub fn is_user_task(id: TaskId) -> bool {
    let slot = table_slot(id.0);
    if TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        TASK_TABLE[slot].user_code_virt.load(Ordering::Relaxed) != 0
    } else {
        false
    }
}

pub fn get_task_user_entry(id: TaskId) -> Option<(u64, u64)> {
    let slot = table_slot(id.0);
    if TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        let entry_rip = TASK_TABLE[slot].user_entry_rip.load(Ordering::Relaxed);
        let user_rsp = TASK_TABLE[slot].user_rsp.load(Ordering::Relaxed);
        if entry_rip != 0 && user_rsp != 0 {
            return Some((entry_rip, user_rsp));
        }
    }
    None
}

pub fn get_task_user_pml4(id: TaskId) -> Option<u64> {
    let slot = table_slot(id.0);
    if TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        let p = TASK_TABLE[slot].user_pml4.load(Ordering::Relaxed);
        if p != 0 {
            return Some(p);
        }
    }
    None
}

pub fn take_task_user_pml4(id: TaskId) -> Option<u64> {
    let slot = table_slot(id.0);
    if TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
        return None;
    }
    let p = TASK_TABLE[slot].user_pml4.swap(0, Ordering::AcqRel);
    if p == 0 {
        None
    } else {
        Some(p)
    }
}

// ============================================================================
// Public API - Priority Management
// ============================================================================

pub fn set_task_priority(id: TaskId, new_prio: u8) -> bool {
    with_interrupts_masked(|| {
        let slot = table_slot(id.0);
        if TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
            return false;
        }
        if TaskState::from_u8(TASK_TABLE[slot].state.load(Ordering::Acquire)) != TaskState::Ready {
            return false;
        }
        TASK_TABLE[slot].priority.store(new_prio, Ordering::Relaxed);
        TASK_TABLE[slot].enqueue_tick.store(ticks(), Ordering::Relaxed);
        TASK_TABLE[slot].slice.store(dispatch::slice_for_priority(new_prio), Ordering::Relaxed);
        true
    })
}

pub fn set_task_priority_any(id: TaskId, new_prio: u8) -> bool {
    with_interrupts_masked(|| {
        let slot = table_slot(id.0);
        if TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
            return false;
        }

        let state = TaskState::from_u8(TASK_TABLE[slot].state.load(Ordering::Acquire));
        if state == TaskState::Empty {
            return false;
        }

        TASK_TABLE[slot].priority.store(new_prio, Ordering::Relaxed);
        TASK_TABLE[slot].slice.store(dispatch::slice_for_priority(new_prio), Ordering::Relaxed);
        if state == TaskState::Ready {
            TASK_TABLE[slot].enqueue_tick.store(ticks(), Ordering::Relaxed);
        }
        true
    })
}

pub fn task_priority(id: TaskId) -> u8 {
    let slot = table_slot(id.0);
    if TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        TASK_TABLE[slot].priority.load(Ordering::Relaxed)
    } else {
        128
    }
}

// ============================================================================
// Public API - Sleep & Waking
// ============================================================================

pub fn sleep_current_for_ticks(ticks: u64) -> bool {
    sleep::sleep_current_for_ticks(ticks)
}

pub fn sleep_current_until_tick(deadline_tick: u64) -> bool {
    sleep::sleep_current_until_tick(deadline_tick)
}

pub fn park_current_task() -> bool {
    sleep::park_current_task()
}

pub fn unpark_task(id: TaskId) -> bool {
    with_interrupts_masked(|| sleep::unpark_task(id))
}

// ============================================================================
// Public API - Signals
// ============================================================================

pub fn task_signal(id: TaskId, bits: u64) -> bool {
    signal::task_signal(id, bits)
}

pub fn task_pending_signals(id: TaskId) -> u64 {
    signal::task_pending_signals(id)
}

pub fn task_clear_signals(id: TaskId, bits: u64) -> u64 {
    signal::task_clear_signals(id, bits)
}

pub fn task_signal_mask(id: TaskId) -> u64 {
    signal::task_signal_mask(id)
}

pub fn task_block_signals(id: TaskId, bits: u64) -> u64 {
    signal::task_block_signals(id, bits)
}

pub fn task_unblock_signals(id: TaskId, bits: u64) -> u64 {
    signal::task_unblock_signals(id, bits)
}

pub fn task_pending_unblocked_signals(id: TaskId) -> u64 {
    signal::task_pending_unblocked_signals(id)
}

pub fn task_take_unblocked_signals(id: TaskId, bits: u64) -> u64 {
    signal::task_take_unblocked_signals(id, bits)
}

pub fn task_wait_signal_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> bool {
    signal::task_wait_signal_until_tick(id, bits, deadline_tick)
}

pub fn task_wait_all_signals_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> bool {
    signal::task_wait_all_signals_until_tick(id, bits, deadline_tick)
}

pub fn task_wait_consume_signal_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> u64 {
    signal::task_wait_consume_signal_until_tick(id, bits, deadline_tick)
}

pub fn task_wait_all_consume_signals_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> u64 {
    signal::task_wait_all_consume_signals_until_tick(id, bits, deadline_tick)
}

pub fn task_wait_signal(id: TaskId, bits: u64) -> bool {
    signal::task_wait_signal(id, bits)
}

pub fn task_wait_consume_signal(id: TaskId, bits: u64) -> u64 {
    signal::task_wait_consume_signal(id, bits)
}

pub fn task_wait_all_consume_signals(id: TaskId, bits: u64) -> u64 {
    signal::task_wait_all_consume_signals(id, bits)
}

// ============================================================================
// Public API - Dispatch & Queueing
// ============================================================================

pub fn enqueue_task(id: TaskId) -> bool {
    with_interrupts_masked(|| {
        let cpu_id = unsafe { crate::arch::x86_64::percpu::cpu_id() };
        dispatch::enqueue_task_to_cpu(id, cpu_id)
    })
}

pub fn dequeue_next() -> Option<TaskId> {
    with_interrupts_masked(|| dispatch::dequeue_next_per_cpu())
}

pub fn dispatch_once() -> bool {
    let task = match dequeue_next() {
        Some(t) => t,
        None => return false,
    };

    let slot = table_slot(task.0);
    let fn_ptr = TASK_TABLE[slot].fn_ptr.load(Ordering::Acquire);
    let owns_slot = TASK_TABLE[slot].id.load(Ordering::Relaxed) == task.0;

    if fn_ptr != 0 && owns_slot {
        stats::record_dispatch();
        set_task_state(task, TaskState::Running);
        CURRENT_TASK.store(task.0, Ordering::Release);
        let percpu = unsafe { crate::arch::x86_64::percpu::this_cpu() };
        percpu.current_task = task.0 as usize;

        let task_rsp = TASK_TABLE[slot].context_rsp.load(Ordering::Acquire);
        let prio = TASK_TABLE[slot].priority.load(Ordering::Relaxed);
        TASK_TABLE[slot].slice.store(dispatch::slice_for_priority(prio), Ordering::Relaxed);
        let was_preempted = TASK_TABLE[slot].preempted.swap(false, Ordering::Relaxed);
        percpu.in_task_dispatch = true;
        IN_TASK_DISPATCH.store(true, Ordering::Release);

        if was_preempted {
            unsafe {
                context::context_switch_to_preempted(&mut percpu.scheduler_rsp, task_rsp);
            }
        } else {
            unsafe {
                context::context_switch(&mut percpu.scheduler_rsp, task_rsp);
            }
        }
        // Returned here after the task yielded, slept, or was preempted.
        let percpu = unsafe { crate::arch::x86_64::percpu::this_cpu() };
        percpu.in_task_dispatch = false;
        percpu.current_task = 0;
        IN_TASK_DISPATCH.store(false, Ordering::Release);

        if task_state(task) == TaskState::Running {
            CURRENT_TASK.compare_exchange(task.0, 0, Ordering::AcqRel, Ordering::Acquire).ok();
            set_task_state(task, TaskState::Ready);
            enqueue_task(task);
            stats::record_requeue();
        }
    } else if owns_slot {
        enqueue_task(task);
    }

    true
}

pub fn runnable_count() -> usize {
    with_interrupts_masked(|| dispatch::runnable_count())
}

pub fn configure_slice_classes(high: u8, normal: u8, low: u8) {
    dispatch::configure_slice_classes(high, normal, low)
}

pub fn debug_slice_for_priority(priority: u8) -> u8 {
    dispatch::slice_for_priority(priority)
}

pub fn configure_aging(enabled: bool, ticks_per_level: u64) {
    dispatch::configure_aging(enabled, ticks_per_level)
}

pub fn debug_aging_enabled() -> bool {
    dispatch::aging_enabled()
}

pub fn debug_aging_ticks_per_level() -> u64 {
    dispatch::aging_ticks_per_level()
}

// ============================================================================
// Public API - Time & Ticks
// ============================================================================

pub fn tick() {
    let now = SCHED_TICKS.fetch_add(1, Ordering::Relaxed).saturating_add(1);

    for slot in 0..TABLE_CAP {
        let state = TASK_TABLE[slot].state.load(Ordering::Acquire);
        if state != TaskState::Sleeping as u8 {
            continue;
        }

        let task_id = TASK_TABLE[slot].id.load(Ordering::Relaxed);
        if task_id == 0 {
            continue;
        }

        let wake_at = TASK_TABLE[slot].wake_tick.load(Ordering::Acquire);
        if wake_at != 0 && wake_at != u64::MAX && wake_at <= now {
            let task = TaskId(task_id);
            if dispatch::enqueue_task_inner(task) {
                TASK_TABLE[slot].wake_tick.store(0, Ordering::Relaxed);
                set_task_state(task, TaskState::Ready);
                stats::record_wake();
            }
        }
    }

    let head = dispatch::RING_HEAD.load(Ordering::Relaxed);
    let tail = dispatch::RING_TAIL.load(Ordering::Acquire);
    if head == tail && !IDLE_DECISION_SEEN.swap(true, Ordering::Relaxed) {
        IDLE_DECISION_PENDING.store(true, Ordering::Relaxed);
    }
}

pub fn ticks() -> u64 {
    SCHED_TICKS.load(Ordering::Relaxed)
}

pub fn take_idle_decision_event() -> bool {
    IDLE_DECISION_PENDING.swap(false, Ordering::Relaxed)
}

pub fn task_count() -> usize {
    let mut count = 0;
    for slot in 0..TABLE_CAP {
        if TASK_TABLE[slot].state.load(Ordering::Relaxed) != TaskState::Empty as u8 {
            count += 1;
        }
    }
    count
}

// ============================================================================
// Public API - Debug & Statistics
// ============================================================================

pub fn debug_invariant_flags() -> u64 {
    with_interrupts_masked(|| {
        let mut flags: u64 = 0;
        let current = CURRENT_TASK.load(Ordering::Acquire);
        let mut running_count: usize = 0;

        for slot in 0..TABLE_CAP {
            let id = TASK_TABLE[slot].id.load(Ordering::Relaxed);
            let state = TaskState::from_u8(TASK_TABLE[slot].state.load(Ordering::Acquire));
            let wake = TASK_TABLE[slot].wake_tick.load(Ordering::Acquire);

            match state {
                TaskState::Empty => {
                    if wake != 0 {
                        flags |= 1 << 1;
                    }
                    if id != 0 {
                        flags |= 1 << 3;
                    }
                }
                TaskState::Ready => {
                    if id == 0 {
                        flags |= 1 << 4;
                    }
                    if wake != 0 {
                        flags |= 1 << 6;
                    }
                }
                TaskState::Running => {
                    running_count = running_count.saturating_add(1);
                    if id == 0 || current != id {
                        flags |= 1 << 2;
                    }
                }
                TaskState::Sleeping => {
                    if id == 0 {
                        flags |= 1 << 4;
                    }
                    if wake == 0 {
                        flags |= 1 << 5;
                    }
                    if id != 0 && dispatch::ring_contains_task_inner(TaskId(id)) {
                        flags |= 1 << 0;
                    }
                }
            }
        }

        if current == 0 {
            if running_count != 0 {
                flags |= 1 << 2;
            }
        } else if running_count != 1 {
            flags |= 1 << 2;
        }

        flags
    })
}

pub fn debug_stats_snapshot() -> stats::SchedulerStats {
    stats::snapshot()
}

pub fn stat_preempt_count() -> u64 {
    stats::preempt_count()
}

pub fn stat_aging_boosts() -> u64 {
    stats::aging_boosts()
}

pub fn stat_max_wait_ticks() -> u64 {
    stats::max_wait_ticks()
}

pub fn stat_park_count() -> u64 {
    stats::park_count()
}

pub fn stat_unpark_count() -> u64 {
    stats::unpark_count()
}

pub fn stat_unpark_fail_count() -> u64 {
    stats::unpark_fail_count()
}

pub fn stat_signal_set_count() -> u64 {
    stats::signal_set_count()
}

pub fn stat_signal_wake_count() -> u64 {
    stats::signal_wake_count()
}

pub fn stat_signal_wake_fail_count() -> u64 {
    stats::signal_wake_fail_count()
}

// ============================================================================
// Public API - Timer IRQ (from ISR)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn timer_irq_inner(task_rsp: u64) -> u64 {
    crate::arch::x86_64::interrupts::increment_timer_ticks();
    tick();

    let percpu = unsafe { crate::arch::x86_64::percpu::this_cpu() };
    if !percpu.in_task_dispatch {
        return 0;
    }
    let current = percpu.current_task as u64;
    if current == 0 {
        return 0;
    }

    let id = TaskId(current);
    let slot = table_slot(id.0);

    let remaining = TASK_TABLE[slot].slice.load(Ordering::Relaxed);
    if remaining > 0 {
        let new_val = remaining - 1;
        TASK_TABLE[slot].slice.store(new_val, Ordering::Relaxed);
        if new_val > 0 {
            return 0;
        }
    }

    TASK_TABLE[slot].context_rsp.store(task_rsp, Ordering::Release);
    TASK_TABLE[slot].preempted.store(true, Ordering::Relaxed);
    percpu.current_task = 0;
    CURRENT_TASK.store(0, Ordering::Release);
    set_task_state(id, TaskState::Ready);
    dispatch::enqueue_task_inner(id);
    percpu.in_task_dispatch = false;
    IN_TASK_DISPATCH.store(false, Ordering::Release);
    stats::record_preempt();

    percpu.scheduler_rsp
}

// ============================================================================
// Public API - Idle Loop
// ============================================================================

pub fn run_idle_loop() -> ! {
    crate::console::log("scheduler: idle loop active");

    loop {
        if !dispatch_once() {
            let next_tick = crate::idle::now_ticks().saturating_add(1);
            crate::idle::idle_until(next_tick);
        }
    }
}


// ============================================================================
// Phase 2.3: Per-CPU Scheduler Support
// ============================================================================

/// Initialize per-core scheduler state for an AP.
/// Call from ap_entry() after GSBASE is set.
pub fn init_per_cpu_scheduler(cpu_id: u32) {
    crate::serial::write_str("scheduler: per-core init cpu_id=");
    crate::serial::write_u32(cpu_id);
    crate::serial::write_line("");
    
    // Each CPU uses the shared ready queue
    // Per-core state: current_task_id, context_rsp stored in percpu
}

/// Run the scheduler loop for this CPU (AP or BSP).
/// Phase 3: Dispatches tasks from per-core ready queue with work-stealing.
/// Never returns - runs until system shutdown.
pub fn run() -> ! {
    let cpu_id = unsafe { 
        crate::arch::x86_64::percpu::cpu_id() as u64
    };
    
    crate::serial::write_str("scheduler: run loop active cpu_id=");
    crate::serial::write_u64(cpu_id);
    crate::serial::write_line("");
    
    // Same as run_idle_loop but per-CPU aware
    loop {
        if !dispatch_once() {
            // No task ready: idle
            let next_tick = crate::idle::now_ticks().saturating_add(1);
            crate::idle::idle_until(next_tick);
        }
    }
}

/// Compatibility wrapper for single-core or BSP entry
pub fn run_idle_loop_compat() -> ! {
    crate::console::log("scheduler: idle loop active (compat)");
    run()
}