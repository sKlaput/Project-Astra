pub mod cpu;
pub mod gdt;
pub mod halt;
pub mod interrupts;
pub mod ring3;
pub mod sysentry;

pub use gdt::{kernel_code_selector, kernel_data_selector, ring3_code_selector, ring3_data_selector};
pub use interrupts::{uptime_ms, timer_hz, wait_until_ticks, sleep_ticks};
pub use halt::power_off;

pub fn init() {
    cpu::early_init();
    gdt::init();
    sysentry::init();
    interrupts::init_legacy_pic_pit();
}