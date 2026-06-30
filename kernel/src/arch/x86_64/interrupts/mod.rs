pub mod exceptions;
pub mod isr;
pub mod pic;

pub use exceptions::{
    arm_ring3_breakpoint_probe, ring3_breakpoint_probe_cs, ring3_breakpoint_probe_hit,
    ring3_breakpoint_probe_rip,
};
pub use isr::LAPIC_EOI_VIRT;
pub use pic::{
    mask_pit_irq, restore_pit_irq, unmask_keyboard_irq, unmask_mouse_irq,
    PIC_MASTER_VECTOR_OFFSET, PIC_SLAVE_VECTOR_OFFSET,
};

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Once;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use x86_64::{PrivilegeLevel, VirtAddr};

/// Vector for the LAPIC timer ISR (just above the legacy PIC range).
pub const LAPIC_TIMER_VECTOR: u8 = 0x40;

const TIMER_IRQ_VECTOR: u8 = PIC_MASTER_VECTOR_OFFSET;
const SPURIOUS_MASTER_IRQ_VECTOR: u8 = PIC_MASTER_VECTOR_OFFSET + 7;
const SPURIOUS_SLAVE_IRQ_VECTOR: u8 = PIC_SLAVE_VECTOR_OFFSET + 7;

const IDT_BRINGUP_STAGE: u8 = 4;

static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
static IDT: Once<&'static InterruptDescriptorTable> = Once::new();

static KEYBOARD_HANDLER: AtomicU64 = AtomicU64::new(0);

pub fn init_legacy_pic_pit() {
    crate::serial::write_line("interrupts: init IDT + legacy PIC/PIT");

    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }

    init_idt_staged(IDT_BRINGUP_STAGE);

    pic::remap_pic(PIC_MASTER_VECTOR_OFFSET, PIC_SLAVE_VECTOR_OFFSET);
    pic::mask_all_irq_lines();
    pic::program_pit_periodic(pic::PIT_TARGET_HZ);

    if IDT_BRINGUP_STAGE >= 4 {
        pic::unmask_timer_irq();
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
        }
        crate::serial::write_line("interrupts: IRQ0 unmasked + sti enabled");
    }

    crate::serial::write_str("interrupts: PIC remapped, PIT ");
    crate::serial::write_u32(pic::PIT_TARGET_HZ);
    if IDT_BRINGUP_STAGE >= 4 {
        crate::serial::write_line(" Hz, staged live IRQ mode");
    } else {
        crate::serial::write_line(" Hz, IRQs masked (staged IDT mode)");
    }
}

/// Load the shared IDT on an application processor without touching PIC/PIT state.
pub fn init_ap_interrupts() {
    if let Some(idt) = IDT.get() {
        idt.load();
        return;
    }
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
                .set_handler_fn(exceptions::breakpoint_handler)
                .set_privilege_level(PrivilegeLevel::Ring3);
            crate::serial::write_line("interrupts: idt stage2 breakpoint set");
        }

        if stage >= 3 {
            unsafe {
                idt.double_fault
                    .set_handler_fn(exceptions::double_fault_handler)
                    .set_stack_index(crate::arch::x86_64::gdt::DOUBLE_FAULT_IST_INDEX);
            }
            idt.general_protection_fault
                .set_handler_fn(exceptions::general_protection_fault_handler);
            idt.page_fault.set_handler_fn(exceptions::page_fault_handler);
            crate::serial::write_line("interrupts: idt stage3 exceptions set");
        }

        if stage >= 4 {
            unsafe {
                idt[TIMER_IRQ_VECTOR].set_handler_addr(VirtAddr::new(
                    isr::timer_interrupt_naked as *const () as u64,
                ));
                idt[LAPIC_TIMER_VECTOR].set_handler_addr(VirtAddr::new(
                    isr::lapic_timer_interrupt_naked as *const () as u64,
                ));
            }
            idt[PIC_MASTER_VECTOR_OFFSET + 1].set_handler_fn(keyboard_irq_handler);
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

extern "x86-interrupt" fn spurious_master_irq_handler(_stack_frame: InterruptStackFrame) {
    pic::send_pic_eoi_master();
}

extern "x86-interrupt" fn spurious_slave_irq_handler(_stack_frame: InterruptStackFrame) {
    pic::send_pic_eoi_slave();
}

/// Register a keyboard scancode handler (called from ISR with one scancode byte).
pub fn register_keyboard_handler(f: fn(u8)) {
    KEYBOARD_HANDLER.store(f as usize as u64, Ordering::Release);
}

extern "x86-interrupt" fn keyboard_irq_handler(_stack_frame: InterruptStackFrame) {
    let scancode = pic::inb(0x60);
    pic::send_pic_eoi_master();
    let handler_addr = KEYBOARD_HANDLER.load(Ordering::Acquire);
    if handler_addr != 0 {
        let f: fn(u8) = unsafe { core::mem::transmute(handler_addr as usize) };
        f(scancode);
    }
}

extern "x86-interrupt" fn mouse_irq_handler(_stack_frame: InterruptStackFrame) {
    pic::send_pic_eoi_slave();
}

pub fn timer_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn increment_timer_ticks() {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn timer_hz() -> u32 {
    pic::PIT_TARGET_HZ
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
    pic::PIT_TARGET_HZ
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
    timer_ticks().saturating_mul(1000) / (pic::PIT_TARGET_HZ as u64)
}

fn interrupts_enabled() -> bool {
    let rflags: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) rflags, options(nomem, preserves_flags));
    }
    (rflags & (1 << 9)) != 0
}
