use super::*;

pub(crate) fn probe_idle_for_ticks() {
    let hz = idle::hz() as u64;
    let duration_ticks = (hz * 80) / 1000;
    let before_ticks = idle::now_ticks();
    let deadline_ticks = before_ticks.saturating_add(duration_ticks);
    idle::idle_until(deadline_ticks);
    let after_ticks = idle::now_ticks();

    serial::write_str("interrupts: idle-ticks before=");
    serial::write_u64(before_ticks);
    serial::write_str(" after=");
    serial::write_u64(after_ticks);
    serial::write_str(" delta=");
    serial::write_u64(after_ticks.saturating_sub(before_ticks));
    serial::write_line("");
}

pub(crate) fn probe_heap_multi_page() {
    use alloc::vec::Vec;

    let mut bytes = Vec::with_capacity(9000);
    bytes.resize(9000, 0xA5);

    serial::write_str("heap: multi-page alloc bytes=");
    serial::write_u64(bytes.len() as u64);
    serial::write_line("");
}

pub(crate) fn probe_heap_mixed_stress() {
    use alloc::vec::Vec;

    let sizes = [64usize, 512, 2048, 4096, 8192, 16384, 3000, 7000, 12000];
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    let mut total_bytes = 0usize;

    for (index, size) in sizes.iter().enumerate() {
        let mut block = Vec::with_capacity(*size);
        block.resize(*size, (index as u8) ^ 0x5A);

        if !block.is_empty() {
            let last = block.len() - 1;
            block[last] ^= 0xFF;
            block[last] ^= 0xFF;
        }

        total_bytes += block.len();
        blocks.push(block);
    }

    serial::write_str("heap: mixed stress blocks=");
    serial::write_u64(blocks.len() as u64);
    serial::write_str(" total-bytes=");
    serial::write_u64(total_bytes as u64);
    serial::write_line("");

    memory::heap::report_heap_telemetry();
}
