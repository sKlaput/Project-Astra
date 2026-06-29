pub fn idle_once() {
    // Safety: halting with interrupts enabled idles until the next interrupt.
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

pub fn halt_loop() -> ! {
    loop {
        idle_once();
    }
}

pub fn idle_until_ticks(target_ticks: u64) {
    super::interrupts::wait_until_ticks(target_ticks);
}

/// Shut down the machine.
/// Works on QEMU (ACPI port 0x604) and common PC firmware (APM/ACPI 0xB004).
pub fn power_off() -> ! {
    unsafe {
        // QEMU ACPI shutdown
        core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16, options(nomem, nostack));
        // Bochs / older QEMU
        core::arch::asm!("out dx, ax", in("dx") 0xB004u16, in("ax") 0x2000u16, options(nomem, nostack));
        // If still running — triple fault by disabling interrupts and halting forever
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
