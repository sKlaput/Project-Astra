use core::panic::PanicInfo;
use core::alloc::Layout;

pub fn handle(info: &PanicInfo<'_>) -> ! {
    crate::serial::write_line("kernel panic");

    if let Some(location) = info.location() {
        crate::serial::write_str("panic location: ");
        crate::serial::write_str(location.file());
        crate::serial::write_str(":");
        crate::serial::write_u32(location.line());
        crate::serial::write_str("\n");
    }

    loop {
        core::hint::spin_loop();
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    if crate::memory::heap::last_alloc_failure_was_injected() {
        crate::serial::write_line("kernel alloc error (intentional probe)");
    } else {
        crate::serial::write_line("kernel alloc error (unexpected)");
    }
    crate::serial::write_str("alloc layout size=");
    crate::serial::write_u64(layout.size() as u64);
    crate::serial::write_str(" align=");
    crate::serial::write_u64(layout.align() as u64);
    crate::serial::write_line("");

    crate::memory::heap::report_heap_telemetry();

    loop {
        core::hint::spin_loop();
    }
}