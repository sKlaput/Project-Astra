pub fn log(message: &str) {
    crate::serial::write_line(message);
    crate::framebuffer::write_line(message);
}