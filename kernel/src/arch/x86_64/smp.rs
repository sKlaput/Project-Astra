use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use limine::mp::Cpu;

use crate::{
    arch::x86_64::{apic, cpu, halt, interrupts},
    boot::protocol,
    serial,
};

static AP_BOOTSTRAP_ARMED: AtomicBool = AtomicBool::new(false);
static AP_STARTED: AtomicUsize = AtomicUsize::new(0);
const AP_EXTRA_VALID_BIT: u64 = 1u64 << 63;
const AP_EXTRA_LAPIC_MISMATCH_BIT: u64 = 1u64 << 62;
const AP_BOOT_WAIT_MS: u64 = 1500;

/// Bring up application processors using Limine's MP request and park them
/// in a simple HLT loop for now.
pub fn init() {
    if AP_BOOTSTRAP_ARMED.swap(true, Ordering::SeqCst) {
        return;
    }

    let mp = match protocol::MP_REQUEST.get_response() {
        Some(response) => response,
        None => {
            serial::write_line("smp: MP request unavailable");
            return;
        }
    };

    let bsp_lapic_id = mp.bsp_lapic_id();
    let cpus = mp.cpus();
    let mut ap_count = 0usize;

    for cpu in cpus {
        if cpu.lapic_id == bsp_lapic_id {
            continue;
        }
        cpu.extra.store(0, Ordering::SeqCst);
        cpu.goto_address.write(ap_entry);
        ap_count += 1;
    }

    if ap_count == 0 {
        serial::write_line("smp: single-core topology");
        return;
    }

    serial::write_str("smp: arming APs count=");
    serial::write_u64(ap_count as u64);
    serial::write_line("");

    let start_ms = interrupts::uptime_ms();
    let deadline_ms = start_ms.saturating_add(AP_BOOT_WAIT_MS);
    while AP_STARTED.load(Ordering::Relaxed) < ap_count && interrupts::uptime_ms() < deadline_ms {
        core::hint::spin_loop();
    }

    let started = AP_STARTED.load(Ordering::Relaxed);
    serial::write_str("smp: APs started=");
    serial::write_u64(started as u64);
    serial::write_str(" expected=");
    serial::write_u64(ap_count as u64);
    if started == ap_count {
        serial::write_line(" OK");
    } else {
        serial::write_line(" partial");
    }

    let mut handshakes = 0usize;
    let mut lapic_mismatches = 0usize;
    for cpu in cpus {
        if cpu.lapic_id == bsp_lapic_id {
            continue;
        }
        let marker = cpu.extra.load(Ordering::SeqCst);
        if (marker & AP_EXTRA_VALID_BIT) != 0 {
            handshakes += 1;
        }
        if (marker & AP_EXTRA_LAPIC_MISMATCH_BIT) != 0 {
            lapic_mismatches += 1;
        }
    }

    serial::write_str("smp: AP handshakes=");
    serial::write_u64(handshakes as u64);
    serial::write_str(" expected=");
    serial::write_u64(ap_count as u64);
    if handshakes == ap_count {
        serial::write_line(" OK");
    } else {
        serial::write_line(" partial");
    }

    serial::write_str("smp: AP lapic-id mismatches=");
    serial::write_u64(lapic_mismatches as u64);
    serial::write_line("");
}

unsafe extern "C" fn ap_entry(cpu: &Cpu) -> ! {
    // Incremental AP bring-up stage: initialize CPU feature state, publish a
    // handshake marker, then park. Higher-level per-core init comes next.
    cpu::early_init();
    interrupts::init_ap_interrupts();
    let current_lapic = apic::lapic_id();
    let mismatch = (current_lapic != cpu.lapic_id) as u64;

    AP_STARTED.fetch_add(1, Ordering::Relaxed);

    // Publish AP identity in the Limine-owned per-CPU extra word so the BSP
    // can inspect per-core state without AP-side serial lock contention.
    cpu.extra.store(
        AP_EXTRA_VALID_BIT
            | (mismatch * AP_EXTRA_LAPIC_MISMATCH_BIT)
            | ((cpu.id as u64) << 32)
            | (((cpu.lapic_id as u64) & 0xFFFF) << 16)
            | ((current_lapic as u64) & 0xFFFF),
        Ordering::SeqCst,
    );

    halt::halt_loop()
}
