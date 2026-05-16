pub fn now_ticks() -> u64 {
    crate::arch::x86_64::interrupts::timer_ticks()
}

pub fn hz() -> u32 {
    crate::arch::x86_64::interrupts::timer_hz()
}

pub fn sleep_for_ticks(duration_ticks: u64) {
    crate::arch::x86_64::interrupts::sleep_ticks(duration_ticks);
}

pub fn idle_until(deadline_ticks: u64) {
    crate::arch::x86_64::halt::idle_until_ticks(deadline_ticks);
}
