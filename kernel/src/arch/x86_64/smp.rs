use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use limine::mp::Cpu;

use crate::{
    arch::x86_64::{apic, cpu, gdt, halt, interrupts},
    boot::protocol,
    serial,
};

static AP_BOOTSTRAP_ARMED: AtomicBool = AtomicBool::new(false);
static AP_STARTED: AtomicUsize = AtomicUsize::new(0);

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
    let deadline_ms = start_ms.saturating_add(250);
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
}

unsafe extern "C" fn ap_entry(cpu: &Cpu) -> ! {
    // Keep AP bring-up minimal: establish the shared kernel tables, record
    // the core, then park until real per-core scheduling arrives.
    cpu::early_init();
    gdt::init_ap();
    interrupts::init_ap_interrupts();

    let _ = apic::lapic_id();
    AP_STARTED.fetch_add(1, Ordering::Relaxed);

    // Publish AP identity in the Limine-owned per-CPU extra word so the BSP
    // can inspect per-core state without AP-side serial lock contention.
    cpu.extra.store(
        ((cpu.id as u64) << 32) | (cpu.lapic_id as u64),
        Ordering::SeqCst,
    );

    halt::halt_loop()
}
