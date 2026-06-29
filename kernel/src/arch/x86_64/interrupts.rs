use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Once;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::{PrivilegeLevel, VirtAddr};

/// Function pointer registered by the keyboard driver.  Stored as a raw
/// u64 (cast from `fn(u8)`) because `AtomicU64` is always available without
/// needing pointer-width atomics.
static KEYBOARD_HANDLER: AtomicU64 = AtomicU64::new(0);

static RING3_BREAKPOINT_PROBE_ACTIVE: AtomicBool = AtomicBool::new(false);
static RING3_BREAKPOINT_PROBE_HIT: AtomicBool = AtomicBool::new(false);
static RING3_BREAKPOINT_PROBE_RIP: AtomicU64 = AtomicU64::new(0);
static RING3_BREAKPOINT_PROBE_CS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Preemptive naked timer ISR.
//
// Stack layout on entry (CPU already pushed iret frame):
//   [RSP]   RIP  [+8] CS  [+16] RFLAGS  [+24] old_RSP  [+32] SS
//
// After saving 15 GPRs the frame becomes (RSP at r15 = lowest addr):
//   [+0]r15 [+8]r14 [+16]r13 [+24]r12 [+32]r11 [+40]r10
//   [+48]r9 [+56]r8 [+64]rdi [+72]rsi [+80]rbp [+88]rbx
//   [+96]rdx [+104]rcx [+112]rax
//   [+120]RIP [+128]CS [+136]RFLAGS [+144]old_RSP [+152]SS
//
// RSP alignment after 15 pushes over the 5-word iret frame is 16-byte
// aligned (5+15=20 words = 160 bytes; 160 mod 16 == 0), which is correct
// for a SysV AMD64 `call` (the call itself pushes an 8-byte return addr).
//
// EOI is sent before calling Rust so the PIC can accept the next IRQ.
// When timer_irq_inner returns non-zero (sched_rsp) rather than 0, the
// task was preempted: tail-jump to context_restore_to which loads the
// scheduler cooperative frame and `ret`s back to dispatch_once.
// ---------------------------------------------------------------------------
core::arch::global_asm!(
    ".intel_syntax noprefix",
    "    .global timer_interrupt_naked",
    "timer_interrupt_naked:",
    "    push rax",
    "    push rcx",
    "    push rdx",
    "    push rbx",
    "    push rbp",
    "    push rsi",
    "    push rdi",
    "    push r8",
    "    push r9",
    "    push r10",
    "    push r11",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    // Send PIC master EOI (port 0x20 = PIC1_COMMAND, value 0x20 = EOI).
    "    mov al, 0x20",
    "    out 0x20, al",
    // Call Rust handler: arg1 (rdi) = current RSP = full preempted frame.
    // timer_irq_inner internally calls tick() which increments SCHED_TICKS.
    // The hardware TIMER_TICKS counter is incremented inside timer_irq_inner
    // via a separate exported Rust function to avoid asm symbol mangling.
    "    mov rdi, rsp",
    "    call timer_irq_inner",
    // rax == 0 → no preemption; restore GPRs and iretq.
    "    test rax, rax",
    "    jz 1f",
    // rax != 0 → scheduler RSP; tail-call context_restore_to (never returns).
    "    mov rdi, rax",
    "    jmp context_restore_to",
    "1:",
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
    fn timer_interrupt_naked();
}

// ---------------------------------------------------------------------------
// LAPIC timer ISR — same shape as the PIT-driven naked ISR but sends LAPIC
// EOI instead of PIC EOI. The LAPIC EOI register virtual address is loaded
// from a globally-accessible variable populated by apic::install_lapic_timer.
// ---------------------------------------------------------------------------
#[unsafe(no_mangle)]
pub static mut LAPIC_EOI_VIRT: u64 = 0;

core::arch::global_asm!(
    ".intel_syntax noprefix",
    "    .global lapic_timer_interrupt_naked",
    "lapic_timer_interrupt_naked:",
    "    push rax",
    "    push rcx",
    "    push rdx",
    "    push rbx",
    "    push rbp",
    "    push rsi",
    "    push rdi",
    "    push r8",
    "    push r9",
    "    push r10",
    "    push r11",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    // EOI: *LAPIC_EOI_VIRT = 0
    "    mov rax, qword ptr [rip + LAPIC_EOI_VIRT]",
    "    mov dword ptr [rax], 0",
    "    mov rdi, rsp",
    "    call timer_irq_inner",
    "    test rax, rax",
    "    jz 1f",
    "    mov rdi, rax",
    "    jmp context_restore_to",
    "1:",
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
    pub fn lapic_timer_interrupt_naked();
}

const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const PIT_CHANNEL0: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;

const PIC_INIT: u8 = 0x11;
const PIC_8086_MODE: u8 = 0x01;
const PIC_EOI: u8 = 0x20;

const PIT_RATE_GENERATOR: u8 = 0x34;
const PIT_BASE_FREQUENCY: u32 = 1_193_182;
const PIT_TARGET_HZ: u32 = 100;
const PIC_MASTER_VECTOR_OFFSET: u8 = 0x20;
const PIC_SLAVE_VECTOR_OFFSET: u8 = 0x28;
const TIMER_IRQ_VECTOR: u8 = PIC_MASTER_VECTOR_OFFSET;
const SPURIOUS_MASTER_IRQ_VECTOR: u8 = PIC_MASTER_VECTOR_OFFSET + 7;
const SPURIOUS_SLAVE_IRQ_VECTOR: u8 = PIC_SLAVE_VECTOR_OFFSET + 7;

// Staged IDT bring-up for safe diagnostics:
// 1 = empty IDT load only
// 2 = add breakpoint handler
// 3 = add #GP/#PF/#DF handlers
// 4 = add IRQ0 handler and enable IRQ0 + sti
const IDT_BRINGUP_STAGE: u8 = 4;

static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
static IDT: Once<&'static InterruptDescriptorTable> = Once::new();

pub fn init_legacy_pic_pit() {
    crate::serial::write_line("interrupts: init IDT + legacy PIC/PIT");

    // Safety: disabling maskable interrupts avoids races while PIC/PIT are reconfigured.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }

    init_idt_staged(IDT_BRINGUP_STAGE);

    remap_pic(PIC_MASTER_VECTOR_OFFSET, PIC_SLAVE_VECTOR_OFFSET);
    mask_all_irq_lines();
    program_pit_periodic(PIT_TARGET_HZ);

    if IDT_BRINGUP_STAGE >= 4 {
        unmask_timer_irq();
        // Safety: IDT includes timer + core exception handlers at stage 4.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
        }
        crate::serial::write_line("interrupts: IRQ0 unmasked + sti enabled");
    }

    crate::serial::write_str("interrupts: PIC remapped, PIT ");
    crate::serial::write_u32(PIT_TARGET_HZ);
    if IDT_BRINGUP_STAGE >= 4 {
        crate::serial::write_line(" Hz, staged live IRQ mode");
    } else {
        crate::serial::write_line(" Hz, IRQs masked (staged IDT mode)");
    }
}

/// Load the shared IDT on an application processor without touching the PIC
/// or PIT state. The AP entry uses this after it has loaded the shared GDT.
pub fn init_ap_interrupts() {
    if let Some(idt) = IDT.get() {
        idt.load();
        return;
    }

    // Fallback path for unexpected bring-up ordering.
    init_idt_staged(IDT_BRINGUP_STAGE);
}

fn init_idt_staged(stage: u8) {
    crate::serial::write_str("interrupts: idt stage=");
    crate::serial::write_u32(stage as u32);
    crate::serial::write_line(" begin");

    crate::serial::write_line("interrupts: idt static access");

    let idt = IDT.call_once(|| {
        let mut idt = Box::new(InterruptDescriptorTable::new());

        if stage >= 2 {
            idt.breakpoint
                .set_handler_fn(breakpoint_handler)
                .set_privilege_level(PrivilegeLevel::Ring3);
            crate::serial::write_line("interrupts: idt stage2 breakpoint set");
        }

        if stage >= 3 {
            unsafe {
                idt.double_fault
                    .set_handler_fn(double_fault_handler)
                    .set_stack_index(crate::arch::x86_64::gdt::DOUBLE_FAULT_IST_INDEX);
            }
            idt.general_protection_fault
                .set_handler_fn(general_protection_fault_handler);
            idt.page_fault.set_handler_fn(page_fault_handler);
            crate::serial::write_line("interrupts: idt stage3 exceptions set");
        }

        if stage >= 4 {
            // SAFETY: timer_interrupt_naked is a correctly formed naked ISR
            // that saves all GPRs, sends EOI, calls timer_irq_inner, then
            // either iretq or tail-calls context_restore_to.
            unsafe {
                idt[TIMER_IRQ_VECTOR]
                    .set_handler_addr(VirtAddr::new(timer_interrupt_naked as *const () as u64));
            }
            // LAPIC timer (vector 0x40) — sibling naked ISR that sends LAPIC
            // EOI instead of PIC EOI. Programmed by apic::install_lapic_timer
            // at runtime; before that the LVT is masked so this vector is dormant.
            unsafe {
                idt[LAPIC_TIMER_VECTOR].set_handler_addr(VirtAddr::new(
                    lapic_timer_interrupt_naked as *const () as u64,
                ));
            }
            // Keyboard IRQ1 (vector 0x21) — handler dispatches to KEYBOARD_HANDLER fn-ptr.
            idt[PIC_MASTER_VECTOR_OFFSET + 1].set_handler_fn(keyboard_irq_handler);
            // Mouse IRQ12 (slave IRQ4, vector PIC_SLAVE_VECTOR_OFFSET+4 = 0x2C).
            idt[PIC_SLAVE_VECTOR_OFFSET + 4].set_handler_fn(mouse_irq_handler);
            idt[SPURIOUS_MASTER_IRQ_VECTOR].set_handler_fn(spurious_master_irq_handler);
            idt[SPURIOUS_SLAVE_IRQ_VECTOR].set_handler_fn(spurious_slave_irq_handler);
            crate::serial::write_line("interrupts: idt stage4 timer+keyboard set");
        }

        Box::leak(idt)
    });

    crate::serial::write_line("interrupts: idt loading");
    idt.load();

    crate::serial::write_line("interrupts: idt loaded");
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    let code_segment = stack_frame.code_segment.0 as u64;
    if (code_segment & 0x3) == 0x3 {
        let probe_active = RING3_BREAKPOINT_PROBE_ACTIVE.load(Ordering::Relaxed);
        if probe_active {
            RING3_BREAKPOINT_PROBE_ACTIVE.store(false, Ordering::Relaxed);
            RING3_BREAKPOINT_PROBE_HIT.store(true, Ordering::Relaxed);
            RING3_BREAKPOINT_PROBE_RIP
                .store(stack_frame.instruction_pointer.as_u64(), Ordering::Relaxed);
            RING3_BREAKPOINT_PROBE_CS.store(code_segment, Ordering::Relaxed);
        }

        let user_task_breakpoint = crate::scheduler::current_task()
            .map(crate::scheduler::is_user_task)
            .unwrap_or(false);

        if probe_active || user_task_breakpoint {
            let saved_rsp = crate::arch::x86_64::ring3::saved_resume_rsp();
            if saved_rsp != 0 {
                unsafe {
                    crate::arch::x86_64::ring3::resume_saved_stack(saved_rsp);
                }
            }
        }
    }

    crate::serial::write_line("interrupts: breakpoint exception");
}

pub fn arm_ring3_breakpoint_probe() {
    RING3_BREAKPOINT_PROBE_HIT.store(false, Ordering::Relaxed);
    RING3_BREAKPOINT_PROBE_RIP.store(0, Ordering::Relaxed);
    RING3_BREAKPOINT_PROBE_CS.store(0, Ordering::Relaxed);
    RING3_BREAKPOINT_PROBE_ACTIVE.store(true, Ordering::Relaxed);
}

pub fn ring3_breakpoint_probe_hit() -> bool {
    RING3_BREAKPOINT_PROBE_HIT.load(Ordering::Relaxed)
}

pub fn ring3_breakpoint_probe_rip() -> u64 {
    RING3_BREAKPOINT_PROBE_RIP.load(Ordering::Relaxed)
}

pub fn ring3_breakpoint_probe_cs() -> u64 {
    RING3_BREAKPOINT_PROBE_CS.load(Ordering::Relaxed)
}

extern "x86-interrupt" fn spurious_master_irq_handler(_stack_frame: InterruptStackFrame) {
    send_pic_eoi_master();
}

extern "x86-interrupt" fn spurious_slave_irq_handler(_stack_frame: InterruptStackFrame) {
    send_pic_eoi_slave();
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    crate::serial::write_line("!!!! DOUBLE FAULT !!!!");
    crate::serial::write_str("RIP: ");
    crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
    crate::serial::write_line("");
    loop {
        core::hint::spin_loop();
    }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) {
    let cpl = stack_frame.code_segment.0 as u64 & 3;

    if cpl == 3 {
        // Ring-3 GP fault: log, kill the user task, resume the scheduler.
        crate::serial::write_str("arch: user-fault gp rip=");
        crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
        crate::serial::write_line("");
        crate::scheduler::abort_current_user_task_from_fault();
    }

    crate::serial::write_line("!!!! GENERAL PROTECTION FAULT !!!!");
    crate::serial::write_str("RIP: ");
    crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
    crate::serial::write_line("");
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let cpl = stack_frame.code_segment.0 as u64 & 3;
    let cr2: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }

    // Bit 0: present (1 = protection violation, 0 = non-present page)
    // Bit 1: write (1 = write access, 0 = read)
    // Bit 2: user (1 = ring-3 access)
    // Bit 3: reserved-bits-set in PTE
    // Bit 4: instruction fetch (NX violation when NXE enabled)
    let ec = error_code.bits();
    let present = (ec & 0x1) != 0;
    let write = (ec & 0x2) != 0;
    let user = (ec & 0x4) != 0;
    let rsvd = (ec & 0x8) != 0;
    let ifetch = (ec & 0x10) != 0;

    if cpl == 3 {
        // Ring-3 page fault: log, kill the user task, resume the scheduler.
        crate::serial::write_str("arch: user-fault pf rip=");
        crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
        crate::serial::write_str(" cr2=");
        crate::serial::write_u64(cr2);
        crate::serial::write_str(" ec=");
        crate::serial::write_u64(ec);
        crate::serial::write_str(" cause=");
        crate::serial::write_str(if !present {
            "not-present"
        } else if rsvd {
            "rsvd-pte"
        } else if ifetch {
            "exec-no-x"
        } else if write {
            "write-violation"
        } else {
            "read-violation"
        });
        crate::serial::write_str(if user { " src=user" } else { " src=kernel" });
        crate::serial::write_line("");
        crate::scheduler::abort_current_user_task_from_fault();
    }

    crate::serial::write_line("!!!! PAGE FAULT !!!!");
    crate::serial::write_str("RIP: ");
    crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
    crate::serial::write_str(" CR2: ");
    crate::serial::write_u64(cr2);
    crate::serial::write_str(" EC: ");
    crate::serial::write_u64(ec);
    crate::serial::write_str(" [");
    if !present {
        crate::serial::write_str("not-present ");
    } else {
        crate::serial::write_str("protection ");
    }
    if write {
        crate::serial::write_str("write ");
    } else {
        crate::serial::write_str("read ");
    }
    if user {
        crate::serial::write_str("user ");
    } else {
        crate::serial::write_str("supervisor ");
    }
    if rsvd {
        crate::serial::write_str("rsvd ");
    }
    if ifetch {
        crate::serial::write_str("ifetch ");
    }
    crate::serial::write_line("]");
}

fn remap_pic(master_offset: u8, slave_offset: u8) {
    let master_mask = inb(PIC1_DATA);
    let slave_mask = inb(PIC2_DATA);

    outb(PIC1_COMMAND, PIC_INIT);
    io_wait();
    outb(PIC2_COMMAND, PIC_INIT);
    io_wait();

    outb(PIC1_DATA, master_offset);
    io_wait();
    outb(PIC2_DATA, slave_offset);
    io_wait();

    outb(PIC1_DATA, 4);
    io_wait();
    outb(PIC2_DATA, 2);
    io_wait();

    outb(PIC1_DATA, PIC_8086_MODE);
    io_wait();
    outb(PIC2_DATA, PIC_8086_MODE);
    io_wait();

    outb(PIC1_DATA, master_mask);
    outb(PIC2_DATA, slave_mask);
}

fn mask_all_irq_lines() {
    outb(PIC1_DATA, 0xFF);
    outb(PIC2_DATA, 0xFF);
}

fn unmask_timer_irq() {
    let master_mask = inb(PIC1_DATA);
    outb(PIC1_DATA, master_mask & !0x01);
}

/// Mask the legacy PIT-driven IRQ0 on the PIC. Used when switching to the
/// LAPIC timer as the tick source.
pub fn mask_pit_irq() {
    let master_mask = inb(PIC1_DATA);
    outb(PIC1_DATA, master_mask | 0x01);
}

/// Re-unmask IRQ0 — restores the PIT as the tick source.
pub fn restore_pit_irq() {
    unmask_timer_irq();
}

/// Vector for the LAPIC timer ISR (just above the legacy PIC range).
pub const LAPIC_TIMER_VECTOR: u8 = 0x40;

/// Unmask IRQ1 (PS/2 keyboard) on the PIC master.
pub fn unmask_keyboard_irq() {
    let master_mask = inb(PIC1_DATA);
    outb(PIC1_DATA, master_mask & !0x02);
}

/// Unmask IRQ12 (PS/2 mouse) on the slave PIC, and unmask IRQ2 (cascade)
/// on the master so slave IRQs reach the CPU.
pub fn unmask_mouse_irq() {
    // Unmask cascade line (IRQ2) on master so slave interrupts get through.
    let master_mask = inb(PIC1_DATA);
    outb(PIC1_DATA, master_mask & !0x04);
    // Unmask IRQ12 (bit 4) on slave.
    let slave_mask = inb(PIC2_DATA);
    outb(PIC2_DATA, slave_mask & !0x10);
}

/// IRQ12 (PS/2 mouse) handler — just acknowledges the PIC so HLT wakes up.
/// Actual byte reading and packet assembly is done by poll_aux_bytes() in the
/// main loop, which is called immediately after HLT returns.
/// Do NOT read from port 0x60 here — that would consume the byte before
/// poll_aux_bytes() can assemble it into a packet.
extern "x86-interrupt" fn mouse_irq_handler(_stack_frame: InterruptStackFrame) {
    send_pic_eoi_slave();
}

/// Register a keyboard scancode handler.  The function is called from the
/// keyboard ISR with one byte of scancode set-1 data.
/// Only one handler can be registered at a time; subsequent calls overwrite.
pub fn register_keyboard_handler(f: fn(u8)) {
    KEYBOARD_HANDLER.store(f as usize as u64, Ordering::Release);
}

/// Keyboard IRQ1 handler.  Reads the scancode from the PS/2 data port,
/// sends PIC EOI, then dispatches to the registered handler if any.
extern "x86-interrupt" fn keyboard_irq_handler(_stack_frame: InterruptStackFrame) {
    // Read scancode from PS/2 data port (0x60) before sending EOI so the
    // keyboard controller does not drop the next scancode.
    let scancode = inb(0x60);
    send_pic_eoi_master();
    let handler_addr = KEYBOARD_HANDLER.load(Ordering::Acquire);
    if handler_addr != 0 {
        // SAFETY: stored via register_keyboard_handler which accepts fn(u8).
        let f: fn(u8) = unsafe { core::mem::transmute(handler_addr as usize) };
        f(scancode);
    }
}

fn program_pit_periodic(target_hz: u32) {
    let divisor_u32 = PIT_BASE_FREQUENCY / target_hz;
    let divisor = u16::try_from(divisor_u32).unwrap_or(u16::MAX);

    outb(PIT_COMMAND, PIT_RATE_GENERATOR);
    outb(PIT_CHANNEL0, (divisor & 0x00FF) as u8);
    outb(PIT_CHANNEL0, ((divisor >> 8) & 0x00FF) as u8);
}

fn send_pic_eoi_master() {
    outb(PIC1_COMMAND, PIC_EOI);
}

fn send_pic_eoi_slave() {
    outb(PIC2_COMMAND, PIC_EOI);
    outb(PIC1_COMMAND, PIC_EOI);
}

pub fn timer_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

/// Increment TIMER_TICKS by one.  Called from timer_irq_inner so the naked
/// ISR does not need to resolve the mangled Rust static symbol.
#[no_mangle]
pub extern "C" fn increment_timer_ticks() {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn timer_hz() -> u32 {
    PIT_TARGET_HZ
}

pub fn legacy_idt_bringup_stage() -> u8 {
    IDT_BRINGUP_STAGE
}

pub fn legacy_timer_vector() -> u8 {
    TIMER_IRQ_VECTOR
}

pub fn legacy_pic_vector_offsets() -> (u8, u8) {
    (PIC_MASTER_VECTOR_OFFSET, PIC_SLAVE_VECTOR_OFFSET)
}

pub fn legacy_spurious_vectors() -> (u8, u8) {
    (SPURIOUS_MASTER_IRQ_VECTOR, SPURIOUS_SLAVE_IRQ_VECTOR)
}

pub fn legacy_pit_target_hz() -> u32 {
    PIT_TARGET_HZ
}

pub fn wait_until_ticks(target_ticks: u64) {
    while timer_ticks() < target_ticks {
        if interrupts_enabled() {
            crate::arch::x86_64::halt::idle_once();
        } else {
            core::hint::spin_loop();
        }
    }
}

pub fn sleep_ticks(duration_ticks: u64) {
    let deadline = timer_ticks().saturating_add(duration_ticks);
    wait_until_ticks(deadline);
}

pub fn uptime_ms() -> u64 {
    timer_ticks().saturating_mul(1000) / (PIT_TARGET_HZ as u64)
}

fn interrupts_enabled() -> bool {
    let rflags: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) rflags, options(nomem, preserves_flags));
    }
    (rflags & (1 << 9)) != 0
}

fn io_wait() {
    outb(0x80, 0);
}

fn outb(port: u16, value: u8) {
    // Safety: this performs privileged x86 port I/O to legacy PIC/PIT hardware registers.
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

fn inb(port: u16) -> u8 {
    let value: u8;

    // Safety: this performs privileged x86 port I/O read from legacy PIC/PIT status registers.
    unsafe {
        core::arch::asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }

    value
}
