pub const PIC1_COMMAND: u16 = 0x20;
pub const PIC1_DATA: u16 = 0x21;
pub const PIC2_COMMAND: u16 = 0xA0;
pub const PIC2_DATA: u16 = 0xA1;

pub const PIT_CHANNEL0: u16 = 0x40;
pub const PIT_COMMAND: u16 = 0x43;

const PIC_INIT: u8 = 0x11;
const PIC_8086_MODE: u8 = 0x01;
pub const PIC_EOI: u8 = 0x20;

const PIT_RATE_GENERATOR: u8 = 0x34;
pub const PIT_BASE_FREQUENCY: u32 = 1_193_182;
pub const PIT_TARGET_HZ: u32 = 100;
pub const PIC_MASTER_VECTOR_OFFSET: u8 = 0x20;
pub const PIC_SLAVE_VECTOR_OFFSET: u8 = 0x28;

pub fn remap_pic(master_offset: u8, slave_offset: u8) {
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

pub fn mask_all_irq_lines() {
    outb(PIC1_DATA, 0xFF);
    outb(PIC2_DATA, 0xFF);
}

pub fn unmask_timer_irq() {
    let master_mask = inb(PIC1_DATA);
    outb(PIC1_DATA, master_mask & !0x01);
}

/// Mask the legacy PIT-driven IRQ0 on the PIC. Used when switching to the LAPIC timer.
pub fn mask_pit_irq() {
    let master_mask = inb(PIC1_DATA);
    outb(PIC1_DATA, master_mask | 0x01);
}

/// Re-unmask IRQ0 — restores the PIT as the tick source.
pub fn restore_pit_irq() {
    unmask_timer_irq();
}

/// Unmask IRQ1 (PS/2 keyboard) on the PIC master.
pub fn unmask_keyboard_irq() {
    let master_mask = inb(PIC1_DATA);
    outb(PIC1_DATA, master_mask & !0x02);
}

/// Unmask IRQ12 (PS/2 mouse) on the slave PIC, and unmask IRQ2 (cascade) on master.
pub fn unmask_mouse_irq() {
    let master_mask = inb(PIC1_DATA);
    outb(PIC1_DATA, master_mask & !0x04);
    let slave_mask = inb(PIC2_DATA);
    outb(PIC2_DATA, slave_mask & !0x10);
}

pub fn program_pit_periodic(target_hz: u32) {
    let divisor_u32 = PIT_BASE_FREQUENCY / target_hz;
    let divisor = u16::try_from(divisor_u32).unwrap_or(u16::MAX);

    outb(PIT_COMMAND, PIT_RATE_GENERATOR);
    outb(PIT_CHANNEL0, (divisor & 0x00FF) as u8);
    outb(PIT_CHANNEL0, ((divisor >> 8) & 0x00FF) as u8);
}

pub fn send_pic_eoi_master() {
    outb(PIC1_COMMAND, PIC_EOI);
}

pub fn send_pic_eoi_slave() {
    outb(PIC2_COMMAND, PIC_EOI);
    outb(PIC1_COMMAND, PIC_EOI);
}

pub fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub fn inb(port: u16) -> u8 {
    let value: u8;
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

pub fn io_wait() {
    outb(0x80, 0);
}
