use alloc::alloc::{alloc, dealloc, Layout};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Cooperative context switch (x86_64, Intel syntax).
// from_rsp : *mut u64 — where to store the current RSP before switching.
// to_rsp   : u64      — the RSP to load (the target task/scheduler context).
// Saves rbp/rbx/r12-r15 + implicit RIP (via call/ret) on the current stack,
// then swaps stacks and restores the saved registers of the target.
// ---------------------------------------------------------------------------
core::arch::global_asm!(
    ".intel_syntax noprefix",
    "    .global context_switch",
    "context_switch:",
    "    push rbp",
    "    push rbx",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    mov qword ptr [rdi], rsp",
    "    mov rsp, rsi",
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbx",
    "    pop rbp",
    "    ret",
    // ----------------------------------------------------------------
    // context_restore_to(sched_rsp: u64) -> !
    // Restores a scheduler cooperative frame without iretq.
    // Used by the preemptive timer ISR to return to dispatch_once.
    // sti re-enables interrupts that the hardware cleared on IRQ entry.
    // ----------------------------------------------------------------
    "    .global context_restore_to",
    "context_restore_to:",
    "    mov rsp, rdi",
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbx",
    "    pop rbp",
    "    sti",
    "    ret",
    // ----------------------------------------------------------------
    // context_switch_to_preempted(from_rsp: *mut u64, to_rsp: u64) -> !
    // Saves the scheduler cooperative frame into *from_rsp, then
    // restores the preempted task's full 15-GPR + iret frame via iretq.
    //
    // ISR GPR frame layout (lowest addr = last pushed by naked ISR):
    //   [+0]r15 [+8]r14 [+16]r13 [+24]r12 [+32]r11 [+40]r10
    //   [+48]r9 [+56]r8 [+64]rdi [+72]rsi [+80]rbp [+88]rbx
    //   [+96]rdx [+104]rcx [+112]rax
    //   [+120]RIP [+128]CS [+136]RFLAGS [+144]RSP_old [+152]SS
    // ----------------------------------------------------------------
    "    .global context_switch_to_preempted",
    "context_switch_to_preempted:",
    "    push rbp",
    "    push rbx",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    mov qword ptr [rdi], rsp",
    "    mov rsp, rsi",
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop r11",
    "    pop r10",
    "    pop r9",
    "    pop r8",
    "    pop rdi",
    "    pop rsi",
    "    pop rbp",
    "    pop rbx",
    "    pop rdx",
    "    pop rcx",
    "    pop rax",
    "    iretq",
    ".att_syntax prefix",
);

extern "C" {
    /// Low-level cooperative context switch.
    /// Saves callee-saved regs + RSP into `*from_rsp`, then loads `to_rsp`
    /// and restores the previously saved regs of the target context.
    fn context_switch(from_rsp: *mut u64, to_rsp: u64);
    /// Restore a cooperative scheduler frame and return to dispatch_once.
    /// Called via tail-jmp from the preemptive timer ISR; never returns.
    fn context_restore_to(sched_rsp: u64) -> !;
    /// Save scheduler cooperative frame then restore a preempted task's
    /// full 15-GPR + iret frame, resuming the task via iretq.
    fn context_switch_to_preempted(from_rsp: *mut u64, to_rsp: u64);
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

        // Switch to the task's user address space before SYSRET to ring-3.
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
        // Back in kernel context after int3/syscall return path.
        unsafe { crate::memory::paging::switch_cr3(kernel_pml4_phys()) };
        // The int3 resume path bypasses a normal x86-interrupt return, so make
        // sure timer IRQs are re-enabled before the task sleeps again.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }

        let _ = sleep_current_for_ticks(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TaskId(pub u64);

/// Encoded as a u8 in TASK_TABLE for atomic storage.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum TaskState {
    Empty = 0,
    Ready = 1,
    Running = 2,
    Sleeping = 3,
}

impl TaskState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => TaskState::Ready,
            2 => TaskState::Running,
            3 => TaskState::Sleeping,
            _ => TaskState::Empty,
        }
    }
}

pub const RING_CAP: usize = 8;
const TABLE_CAP: usize = 16;

pub fn ring_capacity() -> usize {
    RING_CAP
}

// Parallel metadata table: slot = task_id % TABLE_CAP.
// Also stores the owning task_id so stale entries can be detected.
static TASK_TABLE_ID: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
static TASK_TABLE_STATE: [AtomicU8; TABLE_CAP] = [const { AtomicU8::new(0) }; TABLE_CAP];
// 0 = no current task.
static CURRENT_TASK: AtomicU64 = AtomicU64::new(0);

// Function pointer table: stores fn() as u64; 0 means no entry point registered.
static TASK_TABLE_FN: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
// Wake-tick table for sleeping tasks. 0 means "not sleeping".
static TASK_TABLE_WAKE_TICK: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
// Tick when the task was most recently enqueued into the ready ring.
static TASK_TABLE_ENQUEUE_TICK: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
// Task priorities: lower value = higher urgency.  Range 0-255; default = 128.
static TASK_TABLE_PRIORITY: [AtomicU8; TABLE_CAP] = [const { AtomicU8::new(128) }; TABLE_CAP];
// Remaining time-slice ticks for the running task.  Reset on every dispatch.
const DEFAULT_SLICE: u8 = 5;
static SLICE_CLASS_HIGH: AtomicU8 = AtomicU8::new(DEFAULT_SLICE);
static SLICE_CLASS_NORMAL: AtomicU8 = AtomicU8::new(DEFAULT_SLICE);
static SLICE_CLASS_LOW: AtomicU8 = AtomicU8::new(DEFAULT_SLICE);
static TASK_TABLE_SLICE: [AtomicU8; TABLE_CAP] =
    [const { AtomicU8::new(DEFAULT_SLICE) }; TABLE_CAP];
// Set by timer_irq_inner when a task is force-preempted; cleared by dispatch_once.
static TASK_TABLE_PREEMPTED: [AtomicBool; TABLE_CAP] =
    [const { AtomicBool::new(false) }; TABLE_CAP];
// Aging controls for anti-starvation in ready-queue selection.
static AGING_ENABLED: AtomicBool = AtomicBool::new(true);
static AGING_TICKS_PER_LEVEL: AtomicU64 = AtomicU64::new(2);
// Saved RSP for each task slot. Set up on spawn; updated on each context switch.
static TASK_TABLE_CONTEXT: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
// Stack base pointer for each task context. 0 means no allocated stack.
static TASK_TABLE_STACK_BASE: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
// Task name: stored as a raw ptr+len pair so we can reconstruct a &'static str.
// A zero pointer means "unnamed".
static TASK_TABLE_NAME_PTR: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
static TASK_TABLE_NAME_LEN: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
// Pending signals: 64 bits per task (bits 0-63 = event flags).
// Tasks can set/check signals without blocking.
static TASK_TABLE_SIGNALS: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
// Signal mask: 1 bit blocks delivery for the corresponding signal bit.
// A blocked signal still stays pending, but wait APIs ignore it until unmasked.
static TASK_TABLE_SIGNAL_MASK: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
// Scheduler (dispatcher) saved RSP — written by dispatch_once before entering a task.
static SCHEDULER_CONTEXT_RSP: AtomicU64 = AtomicU64::new(0);
// True while a task is currently executing under dispatch_once.
static IN_TASK_DISPATCH: AtomicBool = AtomicBool::new(false);

// User-mode task metadata: stores per-task user-space code/stack mapping info.
// 0 means not a user task.
static TASK_TABLE_USER_CODE_VIRT: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
static TASK_TABLE_USER_STACK_VIRT: [AtomicU64; TABLE_CAP] =
    [const { AtomicU64::new(0) }; TABLE_CAP];
static TASK_TABLE_USER_ENTRY_RIP: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
static TASK_TABLE_USER_RSP: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];
static TASK_TABLE_USER_PML4: [AtomicU64; TABLE_CAP] = [const { AtomicU64::new(0) }; TABLE_CAP];

// Kernel CR3 root captured once and reused when returning from user execution.
static KERNEL_PML4_PHYS: AtomicU64 = AtomicU64::new(0);

/// Size of each task stack in bytes.  Must be a multiple of 16.
const TASK_STACK_SIZE: usize = 8192;

fn table_slot(id: TaskId) -> usize {
    (id.0 as usize) % TABLE_CAP
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
    let slot = table_slot(id);
    TASK_TABLE_ID[slot].store(id.0, Ordering::Release);
    TASK_TABLE_STATE[slot].store(state as u8, Ordering::Release);
}

fn nonzero_slice(v: u8) -> u8 {
    if v == 0 {
        1
    } else {
        v
    }
}

fn slice_for_priority(priority: u8) -> u8 {
    if priority <= 63 {
        nonzero_slice(SLICE_CLASS_HIGH.load(Ordering::Relaxed))
    } else if priority <= 191 {
        nonzero_slice(SLICE_CLASS_NORMAL.load(Ordering::Relaxed))
    } else {
        nonzero_slice(SLICE_CLASS_LOW.load(Ordering::Relaxed))
    }
}

/// Configure preemption slice lengths by priority class.
/// Priority classes: 0..=63 high, 64..=191 normal, 192..=255 low.
/// Any zero input is clamped to 1 tick.
pub fn configure_slice_classes(high: u8, normal: u8, low: u8) {
    SLICE_CLASS_HIGH.store(nonzero_slice(high), Ordering::Relaxed);
    SLICE_CLASS_NORMAL.store(nonzero_slice(normal), Ordering::Relaxed);
    SLICE_CLASS_LOW.store(nonzero_slice(low), Ordering::Relaxed);
}

/// Returns the currently configured slice for a given priority value.
pub fn debug_slice_for_priority(priority: u8) -> u8 {
    slice_for_priority(priority)
}

/// Enable/disable priority aging and set the boost interval in ticks.
/// `ticks_per_level == 0` is clamped to 1.
pub fn configure_aging(enabled: bool, ticks_per_level: u64) {
    AGING_ENABLED.store(enabled, Ordering::Relaxed);
    AGING_TICKS_PER_LEVEL.store(ticks_per_level.max(1), Ordering::Relaxed);
}

pub fn debug_aging_enabled() -> bool {
    AGING_ENABLED.load(Ordering::Relaxed)
}

pub fn debug_aging_ticks_per_level() -> u64 {
    AGING_TICKS_PER_LEVEL.load(Ordering::Relaxed)
}

fn clear_task_state(id: TaskId) {
    let slot = table_slot(id);
    // Only wipe the slot if it still belongs to this task.
    let _ = TASK_TABLE_STATE[slot].compare_exchange(
        TaskState::Ready as u8,
        TaskState::Empty as u8,
        Ordering::Release,
        Ordering::Relaxed,
    );
    let _ = TASK_TABLE_STATE[slot].compare_exchange(
        TaskState::Running as u8,
        TaskState::Empty as u8,
        Ordering::Release,
        Ordering::Relaxed,
    );
    let _ = TASK_TABLE_STATE[slot].compare_exchange(
        TaskState::Sleeping as u8,
        TaskState::Empty as u8,
        Ordering::Release,
        Ordering::Relaxed,
    );
    let _ = TASK_TABLE_ID[slot].compare_exchange(id.0, 0, Ordering::Relaxed, Ordering::Relaxed);
    TASK_TABLE_FN[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_WAKE_TICK[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_ENQUEUE_TICK[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_PRIORITY[slot].store(128, Ordering::Relaxed);
    TASK_TABLE_SLICE[slot].store(slice_for_priority(128), Ordering::Relaxed);
    TASK_TABLE_PREEMPTED[slot].store(false, Ordering::Relaxed);
    TASK_TABLE_NAME_PTR[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_NAME_LEN[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_SIGNALS[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_SIGNAL_MASK[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_CONTEXT[slot].store(0, Ordering::Relaxed);
    let stack_base = TASK_TABLE_STACK_BASE[slot].swap(0, Ordering::Relaxed);
    if stack_base != 0 {
        let layout = Layout::from_size_align(TASK_STACK_SIZE, 16).expect("task stack layout");
        unsafe {
            dealloc(stack_base as *mut u8, layout);
        }
    }
    // Clear user-task metadata
    TASK_TABLE_USER_CODE_VIRT[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_USER_STACK_VIRT[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_USER_ENTRY_RIP[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_USER_RSP[slot].store(0, Ordering::Relaxed);
    TASK_TABLE_USER_PML4[slot].store(0, Ordering::Relaxed);
}

pub fn task_state(id: TaskId) -> TaskState {
    let slot = table_slot(id);
    // Only valid if the slot still belongs to this task.
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        TaskState::from_u8(TASK_TABLE_STATE[slot].load(Ordering::Acquire))
    } else {
        TaskState::Empty
    }
}

pub fn current_task() -> Option<TaskId> {
    let raw = CURRENT_TASK.load(Ordering::Acquire);
    if raw == 0 {
        None
    } else {
        Some(TaskId(raw))
    }
}

/// Return the name label stored for `id`, or `""` if the slot is empty/unnamed.
pub fn task_name(id: TaskId) -> &'static str {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
        return "";
    }
    let ptr = TASK_TABLE_NAME_PTR[slot].load(Ordering::Relaxed) as *const u8;
    let len = TASK_TABLE_NAME_LEN[slot].load(Ordering::Relaxed) as usize;
    if ptr.is_null() || len == 0 {
        return "";
    }
    // SAFETY: stored from a &'static str literal at spawn time; pointer and
    // length are valid for the lifetime of the kernel image.
    unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len)) }
}

static RING_HEAD: AtomicUsize = AtomicUsize::new(0);
static RING_TAIL: AtomicUsize = AtomicUsize::new(0);
static RING_BUF: [AtomicU64; RING_CAP] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);
static SCHED_TICKS: AtomicU64 = AtomicU64::new(0);
static IDLE_DECISION_SEEN: AtomicBool = AtomicBool::new(false);
static IDLE_DECISION_PENDING: AtomicBool = AtomicBool::new(false);
static STAT_DISPATCH_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_SLEEP_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_WAKE_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_EXIT_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_REQUEUE_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_PREEMPT_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_AGING_BOOSTS: AtomicU64 = AtomicU64::new(0);
static STAT_MAX_WAIT_TICKS: AtomicU64 = AtomicU64::new(0);
static STAT_PARK_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_UNPARK_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_UNPARK_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_SIGNAL_SET_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_SIGNAL_WAKE_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_SIGNAL_WAKE_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
pub struct SchedulerStats {
    pub dispatches: u64,
    pub sleeps: u64,
    pub wakes: u64,
    pub exits: u64,
    pub requeues: u64,
    pub preempts: u64,
    pub aging_boosts: u64,
    pub max_wait_ticks: u64,
    pub parks: u64,
    pub unparks: u64,
    pub unpark_fails: u64,
}

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

fn enqueue_task_inner(id: TaskId) -> bool {
    let tail = RING_TAIL.load(Ordering::Relaxed);
    let head = RING_HEAD.load(Ordering::Relaxed);

    if tail.wrapping_sub(head) >= RING_CAP {
        return false;
    }

    RING_BUF[tail % RING_CAP].store(id.0, Ordering::Relaxed);
    TASK_TABLE_ENQUEUE_TICK[table_slot(id)]
        .store(SCHED_TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
    RING_TAIL.store(tail.wrapping_add(1), Ordering::Release);
    IDLE_DECISION_SEEN.store(false, Ordering::Relaxed);
    true
}

/// Dequeue the highest-priority ready task from the ring.
/// Scans all ring entries, picks the one with the lowest priority value
/// (highest urgency), removes it by compacting in-place, and returns it.
/// Equal-priority tasks are selected FIFO (earliest in ring wins).
fn dequeue_next_inner() -> Option<TaskId> {
    let head = RING_HEAD.load(Ordering::Relaxed);
    let tail = RING_TAIL.load(Ordering::Acquire);
    let now = SCHED_TICKS.load(Ordering::Relaxed);

    if head == tail {
        return None;
    }

    // Find the ring index of the highest-priority (min value) entry.
    let mut best_idx = head;
    let mut best_prio = 255u8;

    let mut i = head;
    while i != tail {
        let task_id = RING_BUF[i % RING_CAP].load(Ordering::Relaxed);
        let slot = (task_id as usize) % TABLE_CAP;
        let base = TASK_TABLE_PRIORITY[slot].load(Ordering::Relaxed);
        let effective = if AGING_ENABLED.load(Ordering::Relaxed) {
            let enq = TASK_TABLE_ENQUEUE_TICK[slot].load(Ordering::Relaxed);
            let waited = now.saturating_sub(enq);
            let interval = AGING_TICKS_PER_LEVEL.load(Ordering::Relaxed).max(1);
            let boost = (waited / interval).min(255) as u8;
            if boost > 0 {
                STAT_AGING_BOOSTS.fetch_add(1, Ordering::Relaxed);
                STAT_MAX_WAIT_TICKS.fetch_max(waited, Ordering::Relaxed);
            }
            base.saturating_sub(boost)
        } else {
            base
        };
        if effective < best_prio {
            best_prio = effective;
            best_idx = i;
        }
        i = i.wrapping_add(1);
    }

    let best_id = RING_BUF[best_idx % RING_CAP].load(Ordering::Relaxed);

    // Compact: shift entries after best_idx one position toward head direction.
    let new_tail = tail.wrapping_sub(1);
    let mut j = best_idx;
    while j != new_tail {
        let next_val = RING_BUF[j.wrapping_add(1) % RING_CAP].load(Ordering::Relaxed);
        RING_BUF[j % RING_CAP].store(next_val, Ordering::Relaxed);
        j = j.wrapping_add(1);
    }
    RING_TAIL.store(new_tail, Ordering::Release);

    Some(TaskId(best_id))
}

fn ring_contains_task_inner(id: TaskId) -> bool {
    let head = RING_HEAD.load(Ordering::Relaxed);
    let tail = RING_TAIL.load(Ordering::Acquire);

    let mut idx = head;
    while idx != tail {
        if RING_BUF[idx % RING_CAP].load(Ordering::Relaxed) == id.0 {
            return true;
        }
        idx = idx.wrapping_add(1);
    }

    false
}

// ---------------------------------------------------------------------------
// Per-task stack helpers
// ---------------------------------------------------------------------------

/// Called when a task function returns without calling `exit_task`.
/// Acts as the implicit return address on every new task's initial stack frame.
#[inline(never)]
extern "C" fn task_exit_trampoline() -> ! {
    if let Some(id) = current_task() {
        exit_task(id);
    }
    // Should never reach here; exit_task context-switches away.
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

/// Allocate `TASK_STACK_SIZE` bytes from the kernel heap and initialise an
/// x86_64 context frame so that the first dispatch into this task calls `entry`
/// (and, if `entry` ever returns, falls through to `task_exit_trampoline`).
///
/// Returns the initial RSP value that should be stored in TASK_TABLE_CONTEXT.
///
/// SAFETY: allocates from the global kernel heap; the returned pointer must
/// not be freed while the task slot is live (bump allocator — no-op for free).
fn alloc_task_context(entry: fn()) -> (u64, u64) {
    let layout = Layout::from_size_align(TASK_STACK_SIZE, 16).expect("task stack layout");
    let stack_base = unsafe { alloc(layout) };
    assert!(!stack_base.is_null(), "task stack alloc failed");

    // stack grows DOWN; set up the initial frame starting at the top.
    // The context_switch stub pops: r15, r14, r13, r12, rbx, rbp, then rets.
    // So from rsp upward (low → high address):
    //   [rsp+ 0] r15 = 0
    //   [rsp+ 8] r14 = 0
    //   [rsp+16] r13 = 0
    //   [rsp+24] r12 = 0
    //   [rsp+32] rbx = 0
    //   [rsp+40] rbp = 0
    //   [rsp+48] entry ptr   ← ret jumps here; rsp after ret = rsp+56
    //   [rsp+56] trampoline  ← task's "return address" if entry() returns
    let stack_top = unsafe { (stack_base as *mut u64).add(TASK_STACK_SIZE / 8) };
    unsafe {
        *stack_top.sub(1) = task_exit_trampoline as *const () as usize as u64; // [rsp+56]
        *stack_top.sub(2) = entry as *const () as usize as u64; // [rsp+48]
        *stack_top.sub(3) = 0; // rbp
        *stack_top.sub(4) = 0; // rbx
        *stack_top.sub(5) = 0; // r12
        *stack_top.sub(6) = 0; // r13
        *stack_top.sub(7) = 0; // r14
        *stack_top.sub(8) = 0; // r15
        (stack_top.sub(8) as u64, stack_base as u64)
    }
}

pub fn spawn_task() -> Option<TaskId> {
    let task_id = TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed));

    if enqueue_task(task_id) {
        let slot = table_slot(task_id);
        TASK_TABLE_FN[slot].store(0, Ordering::Relaxed);
        TASK_TABLE_WAKE_TICK[slot].store(0, Ordering::Relaxed);
        set_task_state(task_id, TaskState::Ready);
        Some(task_id)
    } else {
        None
    }
}

/// Mark a task as exited: clears it from the metadata table and from
/// CURRENT_TASK if it was the running task.  When called from within an
/// active dispatch context the function context-switches back to the
/// scheduler and does not return to the caller.
pub fn exit_task(id: TaskId) {
    STAT_EXIT_COUNT.fetch_add(1, Ordering::Relaxed);
    CURRENT_TASK
        .compare_exchange(id.0, 0, Ordering::AcqRel, Ordering::Acquire)
        .ok();
    let slot = table_slot(id);
    clear_task_state(id);

    // If we are currently executing inside dispatch_once (i.e. the call came
    // from task code), switch back to the scheduler context so dispatch_once
    // can resume its loop.  The current task's RSP is saved into its context
    // slot (stale, since the task is gone) then overwritten on next spawn.
    if IN_TASK_DISPATCH.load(Ordering::Acquire) {
        let sched_rsp = SCHEDULER_CONTEXT_RSP.load(Ordering::Acquire);
        unsafe {
            context_switch(TASK_TABLE_CONTEXT[slot].as_ptr(), sched_rsp);
        }
        // Unreachable: empty task is never re-dispatched.
        loop {
            unsafe {
                core::arch::asm!("hlt", options(nomem, nostack));
            }
        }
    }
}

/// Called from a CPU exception handler when the current ring-3 user task
/// triggers a fault (page fault, general-protection fault, etc.).
///
/// Clears the task from the scheduler tables then jumps back to the scheduler
/// loop via `context_restore_to` — i.e., it resumes the instruction in
/// `dispatch_once` that follows the `context_switch` call used to enter the
/// task.  This is the same mechanism used by the preemptive timer ISR.
///
/// The exception handler MUST NOT execute `iretq` after this call.
/// Never returns.
pub fn abort_current_user_task_from_fault() -> ! {
    // Fault handlers may resume scheduler directly; restore kernel address
    // space first so scheduler/core kernel paths do not run under user CR3.
    unsafe { crate::memory::paging::switch_cr3(kernel_pml4_phys()) };

    if let Some(id) = current_task() {
        STAT_EXIT_COUNT.fetch_add(1, Ordering::Relaxed);
        CURRENT_TASK
            .compare_exchange(id.0, 0, Ordering::AcqRel, Ordering::Acquire)
            .ok();
        clear_task_state(id);
    }
    // Restore the scheduler cooperative frame.  dispatch_once will clear
    // IN_TASK_DISPATCH and skip the safety-net requeue (task state is Empty).
    let sched_rsp = SCHEDULER_CONTEXT_RSP.load(Ordering::Acquire);
    if sched_rsp != 0 {
        // SAFETY: sched_rsp was stored by dispatch_once's context_switch call;
        // restoring it resumes that frame without returning through the ISR.
        unsafe { context_restore_to(sched_rsp) }
    } else {
        loop {
            unsafe {
                core::arch::asm!("hlt", options(nomem, nostack));
            }
        }
    }
}

/// Spawn a task with a registered entry-point function.
/// Allocates an 8 KiB stack and initialises the context frame so the first
/// dispatch into this task calls `f` from the top of its own stack.
/// Uses default priority 128 (normal).
pub fn spawn_task_with_fn(f: fn()) -> Option<TaskId> {
    spawn_task_with_fn_prio_name(f, 128, "")
}

/// Spawn a task with an explicit priority.  Lower value = higher urgency (0-255).
pub fn spawn_task_with_fn_prio(f: fn(), priority: u8) -> Option<TaskId> {
    spawn_task_with_fn_prio_name(f, priority, "")
}

/// Spawn a task with an explicit priority and a static name label.
pub fn spawn_task_with_fn_prio_name(f: fn(), priority: u8, name: &'static str) -> Option<TaskId> {
    let task_id = TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed));
    if enqueue_task(task_id) {
        let slot = table_slot(task_id);
        let (initial_rsp, stack_base) = alloc_task_context(f);
        TASK_TABLE_FN[slot].store(f as usize as u64, Ordering::Relaxed);
        TASK_TABLE_CONTEXT[slot].store(initial_rsp, Ordering::Release);
        TASK_TABLE_STACK_BASE[slot].store(stack_base, Ordering::Relaxed);
        TASK_TABLE_WAKE_TICK[slot].store(0, Ordering::Relaxed);
        TASK_TABLE_PRIORITY[slot].store(priority, Ordering::Relaxed);
        TASK_TABLE_SLICE[slot].store(slice_for_priority(priority), Ordering::Relaxed);
        TASK_TABLE_NAME_PTR[slot].store(name.as_ptr() as u64, Ordering::Relaxed);
        TASK_TABLE_NAME_LEN[slot].store(name.len() as u64, Ordering::Relaxed);
        set_task_state(task_id, TaskState::Ready);
        Some(task_id)
    } else {
        None
    }
}

/// Spawn a scheduler-managed user-mode task.
///
/// The task is dispatched on a normal kernel task stack, enters ring 3 using
/// the registered user entry metadata, returns via an `int3` trap, then sleeps
/// for one scheduler tick before repeating.
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

/// Register user-mode code and stack pages for a previously spawned task.
/// Must be called before the task is dispatched.
/// Returns `true` if the registration succeeded; `false` if the task is not Ready
/// or if the slot does not belong to the given task.
pub fn set_task_user_mode(
    id: TaskId,
    code_virt: u64,
    stack_virt: u64,
    entry_rip: u64,
    user_rsp: u64,
) -> bool {
    let slot = table_slot(id);
    // Only update if the slot still belongs to this task and is Ready (not yet dispatched).
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
        return false;
    }
    TASK_TABLE_USER_CODE_VIRT[slot].store(code_virt, Ordering::Relaxed);
    TASK_TABLE_USER_STACK_VIRT[slot].store(stack_virt, Ordering::Relaxed);
    TASK_TABLE_USER_ENTRY_RIP[slot].store(entry_rip, Ordering::Relaxed);
    TASK_TABLE_USER_RSP[slot].store(user_rsp, Ordering::Relaxed);
    // Backward-compatible default: user task uses current address space unless
    // explicitly assigned a dedicated CR3 root by process spawning code.
    TASK_TABLE_USER_PML4[slot].store(
        crate::memory::paging::current_cr3_phys() as u64,
        Ordering::Relaxed,
    );
    true
}

/// Register the page-table root (CR3 PML4 physical address) for a user task.
pub fn set_task_user_pml4(id: TaskId, pml4_phys: u64) -> bool {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
        return false;
    }
    TASK_TABLE_USER_PML4[slot].store(pml4_phys, Ordering::Relaxed);
    true
}

/// Check if a task is registered as a user-mode task.
pub fn is_user_task(id: TaskId) -> bool {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        TASK_TABLE_USER_CODE_VIRT[slot].load(Ordering::Relaxed) != 0
    } else {
        false
    }
}

/// Get user-mode entry point info for a task. Returns (entry_rip, user_rsp) or None.
pub fn get_task_user_entry(id: TaskId) -> Option<(u64, u64)> {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        let entry_rip = TASK_TABLE_USER_ENTRY_RIP[slot].load(Ordering::Relaxed);
        let user_rsp = TASK_TABLE_USER_RSP[slot].load(Ordering::Relaxed);
        if entry_rip != 0 && user_rsp != 0 {
            return Some((entry_rip, user_rsp));
        }
    }
    None
}

/// Get the user task CR3 root physical address for `id`.
pub fn get_task_user_pml4(id: TaskId) -> Option<u64> {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        let p = TASK_TABLE_USER_PML4[slot].load(Ordering::Relaxed);
        if p != 0 {
            return Some(p);
        }
    }
    None
}

/// Atomically take ownership of the user PML4 root for `id`.
/// Returns the previous root and clears the slot to 0.
pub fn take_task_user_pml4(id: TaskId) -> Option<u64> {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
        return None;
    }
    let p = TASK_TABLE_USER_PML4[slot].swap(0, Ordering::AcqRel);
    if p == 0 {
        None
    } else {
        Some(p)
    }
}

/// Put the currently running task to sleep for `ticks` scheduler ticks.
/// Returns false when there is no current task context.
pub fn sleep_current_for_ticks(ticks: u64) -> bool {
    let wake_at = SCHED_TICKS
        .load(Ordering::Relaxed)
        .saturating_add(ticks.max(1));
    sleep_current_until_tick(wake_at)
}

/// Put the currently running task to sleep until `deadline_tick`.
/// Context-switches back to the scheduler immediately; resumes here when
/// tick() re-enqueues the task and dispatch_once switches back in.
/// Returns false when there is no current task context.
pub fn sleep_current_until_tick(deadline_tick: u64) -> bool {
    let id = match current_task() {
        Some(id) => id,
        None => return false,
    };

    let now = SCHED_TICKS.load(Ordering::Relaxed);
    let wake_at = deadline_tick.max(now.saturating_add(1));
    let slot = table_slot(id);

    TASK_TABLE_WAKE_TICK[slot].store(wake_at, Ordering::Release);
    set_task_state(id, TaskState::Sleeping);
    STAT_SLEEP_COUNT.fetch_add(1, Ordering::Relaxed);
    CURRENT_TASK
        .compare_exchange(id.0, 0, Ordering::AcqRel, Ordering::Acquire)
        .ok();

    // Switch to scheduler; this call returns when the task is woken and
    // re-dispatched by dispatch_once.
    let sched_rsp = SCHEDULER_CONTEXT_RSP.load(Ordering::Acquire);
    unsafe {
        context_switch(TASK_TABLE_CONTEXT[slot].as_ptr(), sched_rsp);
    }

    // Resumed: dispatch_once already set state=Running and CURRENT_TASK.
    true
}

/// Dequeue the next ready task and cooperative context-switch into it.
/// The task runs on its own stack until it calls `sleep_current_for_ticks`,
/// `sleep_current_until_tick`, or `exit_task`, any of which context-switches
/// back here so this function can return.  If the ring is empty, returns false.
pub fn dispatch_once() -> bool {
    let task = match dequeue_next() {
        Some(t) => t,
        None => return false,
    };

    let slot = table_slot(task);
    let fn_ptr = TASK_TABLE_FN[slot].load(Ordering::Acquire);
    let owns_slot = TASK_TABLE_ID[slot].load(Ordering::Relaxed) == task.0;

    if fn_ptr != 0 && owns_slot {
        STAT_DISPATCH_COUNT.fetch_add(1, Ordering::Relaxed);
        set_task_state(task, TaskState::Running);
        CURRENT_TASK.store(task.0, Ordering::Release);

        let task_rsp = TASK_TABLE_CONTEXT[slot].load(Ordering::Acquire);
        // Reset the time slice for every dispatch (cooperative or preempted resume).
        let prio = TASK_TABLE_PRIORITY[slot].load(Ordering::Relaxed);
        TASK_TABLE_SLICE[slot].store(slice_for_priority(prio), Ordering::Relaxed);
        // Check whether this task was preempted mid-execution last time.
        let was_preempted = TASK_TABLE_PREEMPTED[slot].swap(false, Ordering::Relaxed);
        IN_TASK_DISPATCH.store(true, Ordering::Release);
        // SAFETY: task_rsp is valid — either alloc_task_context (new task),
        // a cooperative context_switch frame (resumed task), or the full
        // 15-GPR + iret frame saved by the timer ISR (preempted task).
        if was_preempted {
            unsafe {
                context_switch_to_preempted(SCHEDULER_CONTEXT_RSP.as_ptr(), task_rsp);
            }
        } else {
            unsafe {
                context_switch(SCHEDULER_CONTEXT_RSP.as_ptr(), task_rsp);
            }
        }
        IN_TASK_DISPATCH.store(false, Ordering::Release);

        // Back here after: cooperative sleep/exit, OR preemption via context_restore_to.
        // Safety-net re-queue (should not fire in normal operation).
        if task_state(task) == TaskState::Running {
            CURRENT_TASK
                .compare_exchange(task.0, 0, Ordering::AcqRel, Ordering::Acquire)
                .ok();
            set_task_state(task, TaskState::Ready);
            enqueue_task(task);
            STAT_REQUEUE_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    } else if owns_slot {
        // No valid fn pointer for a live slot — preserve the task until a
        // caller either registers an entry point or exits it explicitly.
        enqueue_task(task);
    }

    true
}

pub fn enqueue_task(id: TaskId) -> bool {
    with_interrupts_masked(|| enqueue_task_inner(id))
}

pub fn dequeue_next() -> Option<TaskId> {
    with_interrupts_masked(dequeue_next_inner)
}

/// Change the priority of a task that is currently in the Ready state
/// (i.e. sitting in the ring waiting to be dispatched).
///
/// - Updates the priority table immediately.
/// - Resets the enqueue timestamp to "now" so aging accumulation restarts
///   from a clean baseline at the new priority level.
/// - Adjusts the stored time-slice to match the new priority class.
///
/// Returns `true` if the task was found in Ready state and the update was
/// applied; `false` if the task is not Ready (Running/Sleeping/Empty),
/// or if the id does not match the table slot (stale).
pub fn set_task_priority(id: TaskId, new_prio: u8) -> bool {
    with_interrupts_masked(|| {
        let slot = table_slot(id);
        // Only update if this slot still belongs to `id` and is Ready.
        if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
            return false;
        }
        if TaskState::from_u8(TASK_TABLE_STATE[slot].load(Ordering::Acquire)) != TaskState::Ready {
            return false;
        }
        TASK_TABLE_PRIORITY[slot].store(new_prio, Ordering::Relaxed);
        // Reset enqueue timestamp so aging restarts from 0 at the new level.
        TASK_TABLE_ENQUEUE_TICK[slot].store(SCHED_TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
        // Update the slice so it reflects the new priority class on next dispatch.
        TASK_TABLE_SLICE[slot].store(slice_for_priority(new_prio), Ordering::Relaxed);
        true
    })
}

/// Change the priority of any live task state (Ready/Running/Sleeping).
///
/// For Ready tasks, enqueue timestamp is reset so aging restarts from the new
/// level. For Running/Sleeping tasks, the priority and slice are updated
/// immediately and the enqueue timestamp is left unchanged.
pub fn set_task_priority_any(id: TaskId, new_prio: u8) -> bool {
    with_interrupts_masked(|| {
        let slot = table_slot(id);
        if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
            return false;
        }

        let state = TaskState::from_u8(TASK_TABLE_STATE[slot].load(Ordering::Acquire));
        if state == TaskState::Empty {
            return false;
        }

        TASK_TABLE_PRIORITY[slot].store(new_prio, Ordering::Relaxed);
        TASK_TABLE_SLICE[slot].store(slice_for_priority(new_prio), Ordering::Relaxed);
        if state == TaskState::Ready {
            TASK_TABLE_ENQUEUE_TICK[slot]
                .store(SCHED_TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        true
    })
}

/// Read back the current stored priority for a task.
/// Returns 128 (default) if the slot is empty or stale.
pub fn task_priority(id: TaskId) -> u8 {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        TASK_TABLE_PRIORITY[slot].load(Ordering::Relaxed)
    } else {
        128
    }
}

/// Set one or more signal bits on a task.  Returns whether the task slot was valid.
pub fn task_signal(id: TaskId, bits: u64) -> bool {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        TASK_TABLE_SIGNALS[slot].fetch_or(bits, Ordering::Relaxed);
        STAT_SIGNAL_SET_COUNT.fetch_add(1, Ordering::Relaxed);
        // Best-effort wake for sleepers waiting on signals.
        let state = TaskState::from_u8(TASK_TABLE_STATE[slot].load(Ordering::Acquire));
        if state == TaskState::Sleeping && task_pending_unblocked_signals(id) != 0 {
            if unpark_task(id) {
                STAT_SIGNAL_WAKE_COUNT.fetch_add(1, Ordering::Relaxed);
            } else {
                STAT_SIGNAL_WAKE_FAIL_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }
        true
    } else {
        false
    }
}

/// Read the pending signals for a task.  Returns 0 if the slot is empty or stale.
pub fn task_pending_signals(id: TaskId) -> u64 {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        TASK_TABLE_SIGNALS[slot].load(Ordering::Relaxed)
    } else {
        0
    }
}

/// Clear one or more signal bits on a task.  Returns the state before clearing.
pub fn task_clear_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        TASK_TABLE_SIGNALS[slot].fetch_and(!bits, Ordering::Relaxed)
    } else {
        0
    }
}

/// Read blocked signal mask for a task. 1 bit means blocked.
pub fn task_signal_mask(id: TaskId) -> u64 {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        TASK_TABLE_SIGNAL_MASK[slot].load(Ordering::Relaxed)
    } else {
        0
    }
}

/// Block one or more signal bits for a task. Returns previous mask.
pub fn task_block_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        TASK_TABLE_SIGNAL_MASK[slot].fetch_or(bits, Ordering::Relaxed)
    } else {
        0
    }
}

/// Unblock one or more signal bits for a task. Returns previous mask.
pub fn task_unblock_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) == id.0 {
        TASK_TABLE_SIGNAL_MASK[slot].fetch_and(!bits, Ordering::Relaxed)
    } else {
        0
    }
}

/// Pending signals after applying the task's signal mask.
pub fn task_pending_unblocked_signals(id: TaskId) -> u64 {
    task_pending_signals(id) & !task_signal_mask(id)
}

/// Atomically take currently pending, unblocked matching signal bits.
///
/// Returns the subset of `bits` that was both pending and unblocked at the
/// time of the take, and clears those bits from the pending signal word.
pub fn task_take_unblocked_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
        return 0;
    }

    loop {
        let pending = TASK_TABLE_SIGNALS[slot].load(Ordering::Acquire);
        let mask = TASK_TABLE_SIGNAL_MASK[slot].load(Ordering::Acquire);
        let matched = pending & !mask & bits;
        if matched == 0 {
            return 0;
        }

        if TASK_TABLE_SIGNALS[slot]
            .compare_exchange(
                pending,
                pending & !matched,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            return matched;
        }
    }
}

/// Wait until any bit in `bits` becomes pending for `id`, or until `deadline_tick`.
///
/// Returns `true` if a matching signal is observed before or at the deadline,
/// `false` on timeout.
pub fn task_wait_signal_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> bool {
    loop {
        if task_pending_unblocked_signals(id) & bits != 0 {
            return true;
        }

        let now = ticks();
        if now >= deadline_tick {
            return false;
        }

        if current_task() == Some(id) {
            // Recheck immediately before sleeping to avoid missing a signal that
            // arrives between the loop-top check and the sleep call.
            if task_pending_unblocked_signals(id) & bits != 0 {
                return true;
            }
            sleep_current_until_tick(deadline_tick);
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

/// Wait until all bits in `bits` are pending for `id`, or until `deadline_tick`.
///
/// Returns `true` if all requested bits are observed before or at the
/// deadline, `false` on timeout.
pub fn task_wait_all_signals_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> bool {
    loop {
        if task_pending_unblocked_signals(id) & bits == bits {
            return true;
        }

        let now = ticks();
        if now >= deadline_tick {
            return false;
        }

        if current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits == bits {
                return true;
            }
            sleep_current_until_tick(deadline_tick);
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

/// Wait until any bit in `bits` becomes pending and unblocked for `id`, then
/// consume the matched bits atomically.
///
/// Returns the consumed bit subset, or 0 on timeout.
pub fn task_wait_consume_signal_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> u64 {
    loop {
        let matched = task_take_unblocked_signals(id, bits);
        if matched != 0 {
            return matched;
        }

        let now = ticks();
        if now >= deadline_tick {
            return 0;
        }

        if current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits != 0 {
                continue;
            }
            sleep_current_until_tick(deadline_tick);
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

/// Wait until all bits in `bits` become pending and unblocked for `id`, then
/// consume all requested bits atomically.
///
/// Returns `bits` on success, or 0 on timeout.
pub fn task_wait_all_consume_signals_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> u64 {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
        return 0;
    }

    loop {
        loop {
            let pending = TASK_TABLE_SIGNALS[slot].load(Ordering::Acquire);
            let mask = TASK_TABLE_SIGNAL_MASK[slot].load(Ordering::Acquire);
            let unblocked = pending & !mask;
            if unblocked & bits != bits {
                break;
            }

            if TASK_TABLE_SIGNALS[slot]
                .compare_exchange(
                    pending,
                    pending & !bits,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return bits;
            }
        }

        let now = ticks();
        if now >= deadline_tick {
            return 0;
        }

        if current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits == bits {
                continue;
            }
            sleep_current_until_tick(deadline_tick);
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

/// Wait indefinitely until any bit in `bits` becomes pending for `id`.
///
/// Returns `true` once a matching signal is observed.
pub fn task_wait_signal(id: TaskId, bits: u64) -> bool {
    loop {
        if task_pending_unblocked_signals(id) & bits != 0 {
            return true;
        }

        if current_task() == Some(id) {
            // Recheck before parking to avoid sleeping after a just-arrived signal.
            if task_pending_unblocked_signals(id) & bits != 0 {
                return true;
            }
            park_current_task();
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

/// Wait indefinitely until any bit in `bits` becomes pending and unblocked,
/// then consume the matched bits atomically.
pub fn task_wait_consume_signal(id: TaskId, bits: u64) -> u64 {
    loop {
        let matched = task_take_unblocked_signals(id, bits);
        if matched != 0 {
            return matched;
        }

        if current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits != 0 {
                continue;
            }
            park_current_task();
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

/// Wait indefinitely until all bits in `bits` become pending and unblocked,
/// then consume all requested bits atomically.
pub fn task_wait_all_consume_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table_slot(id);
    if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
        return 0;
    }

    loop {
        loop {
            let pending = TASK_TABLE_SIGNALS[slot].load(Ordering::Acquire);
            let mask = TASK_TABLE_SIGNAL_MASK[slot].load(Ordering::Acquire);
            let unblocked = pending & !mask;
            if unblocked & bits != bits {
                break;
            }

            if TASK_TABLE_SIGNALS[slot]
                .compare_exchange(
                    pending,
                    pending & !bits,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return bits;
            }
        }

        if current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits == bits {
                continue;
            }
            park_current_task();
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

pub fn runnable_count() -> usize {
    with_interrupts_masked(|| {
        let head = RING_HEAD.load(Ordering::Relaxed);
        let tail = RING_TAIL.load(Ordering::Relaxed);
        tail.wrapping_sub(head)
    })
}

/// Returns a bitmask of scheduler invariant violations (0 means clean):
/// bit0: Sleeping task is still in ready ring
/// bit1: Empty slot has non-zero wake deadline
/// bit2: CURRENT_TASK inconsistent with Running state
/// bit3: Empty slot still owns a non-zero task id
/// bit4: Non-empty state has task id == 0
/// bit5: Sleeping task has zero wake deadline
/// bit6: Ready task has non-zero wake deadline
pub fn debug_invariant_flags() -> u64 {
    with_interrupts_masked(|| {
        let mut flags: u64 = 0;
        let current = CURRENT_TASK.load(Ordering::Acquire);
        let mut running_count: usize = 0;

        for slot in 0..TABLE_CAP {
            let id = TASK_TABLE_ID[slot].load(Ordering::Relaxed);
            let state = TaskState::from_u8(TASK_TABLE_STATE[slot].load(Ordering::Acquire));
            let wake = TASK_TABLE_WAKE_TICK[slot].load(Ordering::Acquire);

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
                    // wake == u64::MAX means "parked" (no timer deadline); that is valid.
                    if wake == 0 {
                        flags |= 1 << 5;
                    }
                    if id != 0 && ring_contains_task_inner(TaskId(id)) {
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

/// Called from IRQ0 handler — no serial I/O allowed here.
pub fn tick() {
    let now = SCHED_TICKS
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);

    // Wake any sleeping tasks whose deadline has been reached.
    // Tasks sleeping with wake_at == u64::MAX are "parked" — only woken by
    // an explicit unpark_task() call, not by the timer.
    for slot in 0..TABLE_CAP {
        let state = TASK_TABLE_STATE[slot].load(Ordering::Acquire);
        if state != TaskState::Sleeping as u8 {
            continue;
        }

        let task_id = TASK_TABLE_ID[slot].load(Ordering::Relaxed);
        if task_id == 0 {
            continue;
        }

        let wake_at = TASK_TABLE_WAKE_TICK[slot].load(Ordering::Acquire);
        if wake_at != 0 && wake_at != u64::MAX && wake_at <= now {
            let task = TaskId(task_id);
            if enqueue_task_inner(task) {
                TASK_TABLE_WAKE_TICK[slot].store(0, Ordering::Relaxed);
                set_task_state(task, TaskState::Ready);
                STAT_WAKE_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    // Dispatch happens from normal context via dispatch_once().
    // tick() only sets the idle-decision flag when the ring is empty.
    let head = RING_HEAD.load(Ordering::Relaxed);
    let tail = RING_TAIL.load(Ordering::Acquire);
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

/// Returns the count of tasks currently in the system (any state).
pub fn task_count() -> usize {
    let mut count = 0;
    for slot in 0..TABLE_CAP {
        if TASK_TABLE_STATE[slot].load(Ordering::Relaxed) != TaskState::Empty as u8 {
            count += 1;
        }
    }
    count
}

/// Called from the naked timer ISR — runs with interrupts disabled.
/// Handles all tick work (waking sleepers) then checks whether the current
/// task's time slice has expired.
///
/// Returns 0 if no preemption is needed, or the scheduler RSP value if the
/// caller should tail-jump to `context_restore_to` to swap back to the
/// scheduler stack.
///
/// # Safety
/// Must only be called from the timer ISR with a valid full-GPR save frame
/// at `task_rsp` on the interrupted task's stack.
#[no_mangle]
pub unsafe extern "C" fn timer_irq_inner(task_rsp: u64) -> u64 {
    // Increment the hardware tick counter (for uptime_ms / sleep_ticks).
    crate::arch::x86_64::interrupts::increment_timer_ticks();
    // Run all the normal per-tick work (wake sleepers, idle decision).
    tick();

    // Preemption only makes sense when a task is currently dispatched.
    if !IN_TASK_DISPATCH.load(Ordering::Acquire) {
        return 0;
    }
    let current = CURRENT_TASK.load(Ordering::Acquire);
    if current == 0 {
        return 0;
    }

    let id = TaskId(current);
    let slot = table_slot(id);

    // Decrement slice; if it hits 0 we preempt.
    let remaining = TASK_TABLE_SLICE[slot].load(Ordering::Relaxed);
    if remaining > 0 {
        let new_val = remaining - 1;
        TASK_TABLE_SLICE[slot].store(new_val, Ordering::Relaxed);
        if new_val > 0 {
            return 0; // slice not yet expired
        }
    }

    // Slice expired — preempt the task.
    TASK_TABLE_CONTEXT[slot].store(task_rsp, Ordering::Release);
    TASK_TABLE_PREEMPTED[slot].store(true, Ordering::Relaxed);
    CURRENT_TASK.store(0, Ordering::Release);
    set_task_state(id, TaskState::Ready);
    enqueue_task_inner(id); // already in ISR → interrupts disabled, safe to call directly
    IN_TASK_DISPATCH.store(false, Ordering::Release);
    STAT_PREEMPT_COUNT.fetch_add(1, Ordering::Relaxed);

    SCHEDULER_CONTEXT_RSP.load(Ordering::Acquire) // non-zero → caller should context_restore_to
}

pub fn debug_stats_snapshot() -> SchedulerStats {
    SchedulerStats {
        dispatches: STAT_DISPATCH_COUNT.load(Ordering::Relaxed),
        sleeps: STAT_SLEEP_COUNT.load(Ordering::Relaxed),
        wakes: STAT_WAKE_COUNT.load(Ordering::Relaxed),
        exits: STAT_EXIT_COUNT.load(Ordering::Relaxed),
        requeues: STAT_REQUEUE_COUNT.load(Ordering::Relaxed),
        preempts: STAT_PREEMPT_COUNT.load(Ordering::Relaxed),
        aging_boosts: STAT_AGING_BOOSTS.load(Ordering::Relaxed),
        max_wait_ticks: STAT_MAX_WAIT_TICKS.load(Ordering::Relaxed),
        parks: STAT_PARK_COUNT.load(Ordering::Relaxed),
        unparks: STAT_UNPARK_COUNT.load(Ordering::Relaxed),
        unpark_fails: STAT_UNPARK_FAIL_COUNT.load(Ordering::Relaxed),
    }
}

pub fn stat_preempt_count() -> u64 {
    STAT_PREEMPT_COUNT.load(Ordering::Relaxed)
}

pub fn stat_aging_boosts() -> u64 {
    STAT_AGING_BOOSTS.load(Ordering::Relaxed)
}

pub fn stat_max_wait_ticks() -> u64 {
    STAT_MAX_WAIT_TICKS.load(Ordering::Relaxed)
}

pub fn stat_park_count() -> u64 {
    STAT_PARK_COUNT.load(Ordering::Relaxed)
}

pub fn stat_unpark_count() -> u64 {
    STAT_UNPARK_COUNT.load(Ordering::Relaxed)
}

pub fn stat_unpark_fail_count() -> u64 {
    STAT_UNPARK_FAIL_COUNT.load(Ordering::Relaxed)
}

pub fn stat_signal_set_count() -> u64 {
    STAT_SIGNAL_SET_COUNT.load(Ordering::Relaxed)
}

pub fn stat_signal_wake_count() -> u64 {
    STAT_SIGNAL_WAKE_COUNT.load(Ordering::Relaxed)
}

pub fn stat_signal_wake_fail_count() -> u64 {
    STAT_SIGNAL_WAKE_FAIL_COUNT.load(Ordering::Relaxed)
}

/// Suspend the current task indefinitely (no timer deadline).
/// The task will only be woken by an explicit `unpark_task()` call.
/// Returns false if called outside a dispatched task context.
pub fn park_current_task() -> bool {
    let id = match current_task() {
        Some(id) => id,
        None => return false,
    };

    let slot = table_slot(id);
    // u64::MAX sentinel: tick() skips this entry; only unpark_task() wakes it.
    TASK_TABLE_WAKE_TICK[slot].store(u64::MAX, Ordering::Release);
    set_task_state(id, TaskState::Sleeping);
    STAT_PARK_COUNT.fetch_add(1, Ordering::Relaxed);
    STAT_SLEEP_COUNT.fetch_add(1, Ordering::Relaxed);
    CURRENT_TASK
        .compare_exchange(id.0, 0, Ordering::AcqRel, Ordering::Acquire)
        .ok();

    let sched_rsp = SCHEDULER_CONTEXT_RSP.load(Ordering::Acquire);
    unsafe {
        context_switch(TASK_TABLE_CONTEXT[slot].as_ptr(), sched_rsp);
    }

    // Resumed by unpark_task + dispatch_once.
    true
}

/// Wake a parked (or sleeping) task by id, making it Ready.
/// Safe to call from normal context (interrupts masked internally).
/// Returns true if the task was successfully re-enqueued.
pub fn unpark_task(id: TaskId) -> bool {
    with_interrupts_masked(|| {
        let slot = table_slot(id);
        if TASK_TABLE_ID[slot].load(Ordering::Relaxed) != id.0 {
            STAT_UNPARK_FAIL_COUNT.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let state = TaskState::from_u8(TASK_TABLE_STATE[slot].load(Ordering::Acquire));
        if state != TaskState::Sleeping {
            STAT_UNPARK_FAIL_COUNT.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        if enqueue_task_inner(id) {
            TASK_TABLE_WAKE_TICK[slot].store(0, Ordering::Relaxed);
            set_task_state(id, TaskState::Ready);
            STAT_WAKE_COUNT.fetch_add(1, Ordering::Relaxed);
            STAT_UNPARK_COUNT.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            STAT_UNPARK_FAIL_COUNT.fetch_add(1, Ordering::Relaxed);
            false
        }
    })
}

pub fn run_idle_loop() -> ! {
    crate::console::log("scheduler: idle loop active");

    loop {
        if !dispatch_once() {
            // No runnable tasks — wait for the next timer tick.
            let next_tick = crate::idle::now_ticks().saturating_add(1);
            crate::idle::idle_until(next_tick);
        }
    }
}
