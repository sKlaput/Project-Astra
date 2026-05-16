pub fn init() {
    crate::console::log("boot: limine handoff active");

    let fb_present = crate::boot::protocol::framebuffer_info().is_some();
    if fb_present {
        crate::serial::write_line("boot: framebuffer detected (init deferred until heap ready)");
    } else {
        crate::serial::write_line("boot: framebuffer unavailable");
    }
}

pub mod protocol;