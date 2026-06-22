use crate::{arch, serial};

pub(crate) fn probe_lapic_timer_switch() {
    use arch::x86_64::{apic, interrupts};

    if !apic::lapic_calibrated() {
        serial::write_line("lapic-timer: skip (not calibrated)");
        return;
    }

    // PIT baseline: count how many ticks accumulate in ~250ms.
    let t0_ticks = interrupts::timer_ticks();
    let t0_ms = interrupts::uptime_ms();
    let target_pit = t0_ms + 250;
    while interrupts::uptime_ms() < target_pit {
        core::hint::spin_loop();
    }
    let pit_delta = interrupts::timer_ticks() - t0_ticks;

    if !apic::install_lapic_timer(100) {
        serial::write_line("lapic-timer: install FAILED");
        return;
    }

    // Cycle-bounded LAPIC measurement so a silent ISR cannot hang boot.
    let l0_ticks = interrupts::timer_ticks();
    let l0_ms = interrupts::uptime_ms();
    let target_lapic = l0_ms + 250;
    let spin_cap: u64 = 4_000_000_000;
    let mut spins: u64 = 0;
    let mut silent = false;
    while interrupts::uptime_ms() < target_lapic {
        core::hint::spin_loop();
        spins = spins.wrapping_add(1);
        if spins > spin_cap {
            silent = true;
            break;
        }
    }
    let lapic_delta = interrupts::timer_ticks() - l0_ticks;

    let _ = apic::uninstall_lapic_timer();

    let mut buf = [0u8; 96];
    let mut p = 0usize;
    let prefix = b"lapic-timer: pit_delta=";
    buf[..prefix.len()].copy_from_slice(prefix);
    p += prefix.len();
    p += write_dec_u64(&mut buf[p..], pit_delta);
    let mid = b" lapic_delta=";
    buf[p..p + mid.len()].copy_from_slice(mid);
    p += mid.len();
    p += write_dec_u64(&mut buf[p..], lapic_delta);
    let tag: &[u8] = if silent {
        b" status=SILENT"
    } else if lapic_delta > 0
        && (lapic_delta as i64 - pit_delta as i64).abs() <= (pit_delta as i64 / 4 + 5)
    {
        b" status=OK"
    } else {
        b" status=DIVERGED"
    };
    buf[p..p + tag.len()].copy_from_slice(tag);
    p += tag.len();
    let line = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
    serial::write_line(line);
}

fn write_dec_u64(buf: &mut [u8], mut n: u64) -> usize {
    if n == 0 {
        if !buf.is_empty() {
            buf[0] = b'0';
            return 1;
        }
        return 0;
    }
    let mut tmp = [0u8; 20];
    let mut i = 0;
    while n > 0 {
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    let len = i.min(buf.len());
    for j in 0..len {
        buf[j] = tmp[i - 1 - j];
    }
    len
}
