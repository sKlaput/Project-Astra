use core::alloc::Layout;
use core::panic::PanicInfo;

pub fn handle(info: &PanicInfo<'_>) -> ! {
    crate::serial::write_line("kernel panic");

    // Draw a visible red panic screen so the user sees something meaningful
    // instead of a frozen black display.
    crate::framebuffer::clear(0xAA0000);
    crate::framebuffer::draw_text_scaled(40, 40, "KERNEL PANIC", 0xFFFFFF, 3);
    crate::framebuffer::draw_text_scaled(
        40,
        100,
        "The system has encountered a fatal error.",
        0xFFCCCC,
        2,
    );
    crate::framebuffer::draw_text_scaled(40, 140, "Please restart QEMU.", 0xFFCCCC, 2);

    if let Some(location) = info.location() {
        crate::serial::write_str("panic location: ");
        crate::serial::write_str(location.file());
        crate::serial::write_str(":");
        crate::serial::write_u32(location.line());
        crate::serial::write_str("\n");

        // Show file and line on screen
        let mut line_buf = [0u8; 120];
        let mut pos = 0usize;
        for &b in location.file().as_bytes() {
            if pos >= line_buf.len() - 12 {
                break;
            }
            line_buf[pos] = b;
            pos += 1;
        }
        if pos < line_buf.len() {
            line_buf[pos] = b':';
            pos += 1;
        }
        let mut n = location.line();
        if n == 0 {
            if pos < line_buf.len() {
                line_buf[pos] = b'0';
                pos += 1;
            }
        } else {
            let mut tmp = [0u8; 10];
            let mut ti = 0;
            while n > 0 && ti < tmp.len() {
                tmp[ti] = b'0' + (n % 10) as u8;
                ti += 1;
                n /= 10;
            }
            tmp[..ti].reverse();
            for &b in &tmp[..ti] {
                if pos < line_buf.len() {
                    line_buf[pos] = b;
                    pos += 1;
                }
            }
        }
        if let Ok(s) = core::str::from_utf8(&line_buf[..pos]) {
            crate::framebuffer::draw_text_scaled(40, 190, s, 0xFFFF88, 1);
        }
    }

    crate::framebuffer::present_full();

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
