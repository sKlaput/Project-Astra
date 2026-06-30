//! Low-level context switching and task stack allocation.
//! 
//! Contains x86_64 assembly for cooperative and preemptive context switches,
//! and helpers for initializing task stack frames.

use alloc::alloc::{alloc, dealloc, Layout};

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
    pub fn context_switch(from_rsp: *mut u64, to_rsp: u64);
    /// Restore a cooperative scheduler frame and return to dispatch_once.
    /// Called via tail-jmp from the preemptive timer ISR; never returns.
    pub fn context_restore_to(sched_rsp: u64) -> !;
    /// Save scheduler cooperative frame then restore a preempted task's
    /// full 15-GPR + iret frame, resuming the task via iretq.
    pub fn context_switch_to_preempted(from_rsp: *mut u64, to_rsp: u64);
}

const TASK_STACK_SIZE: usize = 8192;

/// Called when a task function returns without calling `exit_task`.
/// Acts as the implicit return address on every new task's initial stack frame.
#[inline(never)]
extern "C" fn task_exit_trampoline() -> ! {
    if let Some(id) = crate::scheduler::current_task() {
        crate::scheduler::exit_task(id);
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
pub fn alloc_task_context(entry: fn()) -> (u64, u64) {
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

/// Deallocate a task stack by address and size.
pub fn dealloc_task_stack(stack_base: u64) {
    if stack_base != 0 {
        let layout = Layout::from_size_align(TASK_STACK_SIZE, 16).expect("task stack layout");
        unsafe {
            dealloc(stack_base as *mut u8, layout);
        }
    }
}
