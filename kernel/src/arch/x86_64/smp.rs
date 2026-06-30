use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use limine::mp::Cpu;

use crate::{
    arch::x86_64::{apic, cpu, gdt, interrupts},
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
    let cpu_count = cpus.len();
    let mut ap_count = 0usize;

    // Phase 2: Initialize multi-core GDT system with detected CPU count
    gdt::init_multicore_gdt(cpu_count);

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

    if handshakes != ap_count {
        for cpu in cpus {
            if cpu.lapic_id == bsp_lapic_id {
                continue;
            }
            serial::write_str("smp: ap marker lapic=");
            serial::write_u64(cpu.lapic_id as u64);
            serial::write_str(" extra=0x");
            serial::write_hex64(cpu.extra.load(Ordering::SeqCst));
            serial::write_line("");
        }
    }
}

unsafe extern "C" fn ap_entry(cpu: &Cpu) -> ! {
    // Phase 2.3: Complete AP bring-up by integrating with scheduler.
    // Sequence:
    // 1. Early CPU init (FPU/SSE, protections)
    // 2. Get LAPIC ID
    // 3. Load per-core GDT/TSS
    // 4. Set GSBASE for per-core local storage
    // 5. Load IDT
    // 6. Initialize scheduler state
    // 7. Enter scheduler loop (runs tasks)
    
    cpu::early_init();
    let current_lapic = apic::lapic_id();
    
    // Load per-core GDT/TSS and get GSBASE address
    let gsbase_addr = gdt::init_ap_per_core(current_lapic);
    
    // Set GSBASE to enable per-core local storage
    unsafe {
        cpu::set_gsbase(gsbase_addr);
    }
    
    // Load IDT for this CPU
    interrupts::init_ap_interrupts();
    
    // Phase 2.3: Initialize per-core scheduler state
    crate::scheduler::init_per_cpu_scheduler(current_lapic);
    
    let mismatch = (current_lapic != cpu.lapic_id) as u64;

    // Signal that this AP has started
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

    // Phase 2.3: Instead of halting, enter the scheduler loop
    // This allows APs to execute tasks from the shared ready queue
    crate::scheduler::run()  // Never returns
}