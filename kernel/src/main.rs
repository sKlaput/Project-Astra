#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

mod about;
mod acpi;
mod app;
mod arch;
mod boot;
mod calculator;
mod console;
mod desktop;
mod drivers;
mod editor;
mod filemanager;
mod framebuffer;
mod fs;
mod idle;
mod imageviewer;
mod input;
mod loader;
mod logviewer;
mod memory;
mod net;
mod notes;
mod panic;
mod poste14_gui_probes;
mod process;
mod rtc;
mod scheduler;
mod serial;
mod settings;
mod snake;
mod splash;
mod subsystem_validation;
mod sync;
mod syscall;
mod sysmonitor;
mod terminal;
mod tetris;
mod user;

// Boot phase orchestration: organizes probes by E-series phase
mod boot_phases;
mod boot_probes;
mod fat32;

use core::panic::PanicInfo;

#[global_allocator]
static GLOBAL_ALLOCATOR: memory::heap::KernelAllocator = memory::heap::KernelAllocator;

#[unsafe(no_mangle)]
pub extern "C" fn kmain() -> ! {
    // Enable FPU/SSE immediately — the compiler may emit SSE instructions
    // anywhere (x86-64 baseline includes SSE2).
    arch::x86_64::cpu::early_init();

    serial::init();
    console::log("kernel: boot entry reached");
    console::log("kernel: phase E1 skeleton active");

    {
        let cr0 = arch::x86_64::cpu::cr0();
        let cr4 = arch::x86_64::cpu::cr4();
        serial::write_str("kernel: protections WP=");
        serial::write_u64(((cr0 >> 16) & 1) as u64);
        serial::write_str(" SMEP=");
        serial::write_u64(((cr4 >> 20) & 1) as u64);
        serial::write_str(" SMAP=");
        serial::write_u64(((cr4 >> 21) & 1) as u64);
        serial::write_str(" UMIP=");
        serial::write_u64(((cr4 >> 11) & 1) as u64);
        serial::write_str(" smep_avail=");
        serial::write_u64(arch::x86_64::cpu::has_smep() as u64);
        serial::write_str(" smap_avail=");
        serial::write_u64(arch::x86_64::cpu::has_smap() as u64);
        serial::write_str(" umip_avail=");
        serial::write_u64(arch::x86_64::cpu::has_umip() as u64);
        serial::write_line("");
    }

    arch::x86_64::apic::log_summary();
    if !boot::protocol::limine_revision_supported() {
        console::log("kernel: unsupported limine revision");
        arch::x86_64::halt::halt_loop();
    }

    boot::init();
    memory::init_from_boot();
    arch::x86_64::init();

    // ACPI MADT discovery: read-only walk of RSDP -> RSDT/XSDT -> APIC. Used
    // by future LAPIC-timer/IO-APIC slices; safe to fail.
    if acpi::init() {
        acpi::log_summary();
    } else {
        serial::write_line("acpi: madt not found");
    }

    // Now that the heap is available, init framebuffer (allocates backbuffer)
    if framebuffer::init_from_boot() {
        serial::write_line("framebuffer: initialized with backbuffer");
    } else {
        serial::write_line("framebuffer: not available");
    }

    // Init input drivers: keyboard (IRQ1) and PS/2 mouse (polling)
    {
        use drivers::keyboard::Ps2KeyboardDriver;
        use drivers::mouse::Ps2MouseDriver;
        static KB: Ps2KeyboardDriver = Ps2KeyboardDriver;
        static MS: Ps2MouseDriver = Ps2MouseDriver;
        let _ = drivers::Driver::init(&KB);
        let _ = drivers::Driver::init(&MS);
    }

    // Initialise virtio-blk persistent storage
    drivers::virtio_blk::init();
    // Initialise virtio-net NIC (optional — continues if absent)
    drivers::virtio_net::init();
    // Bring up IP stack (static QEMU config: 10.0.2.15/24, gw 10.0.2.2)
    net::init();
    // Mount FAT32 filesystem; format on first boot if blank
    if !fat32::mount() && drivers::virtio_blk::sector_count() > 0 {
        crate::serial::write_line("fat32: blank disk detected, running mkfs...");
        if fat32::mkfs() {
            fat32::mount();
        }
    }

    // Boot splash (presented immediately)
    splash::draw_boot_splash();

    // Execute boot phases in sequence
    boot_phases::phase_e1_e2_core();
    arch::x86_64::smp::init();
    boot_phases::phase_e2_e3_scheduler();
    boot_phases::run_deferred_optional_phases();

    // Calibrate the Local APIC timer against the PIT-driven uptime clock.
    // Read-only at this stage; the scheduler tick source is unchanged.
    arch::x86_64::apic::calibrate_timer();
    arch::x86_64::apic::log_calibration();

    // Boot-time LAPIC timer probe: briefly switch the scheduler tick source
    // from PIT to LAPIC and back, logging the observed delta. Failure here
    // does not abort boot — PIT remains active.
    #[cfg(feature = "boot_probes")] { boot_probes::probe_lapic_timer_switch(); }

    #[cfg(feature = "boot_probes")] { if boot_probes::HEAP_ALLOC_FAILURE_PROBE {
        boot_probes::probe_alloc_failure_path();
    } }

    #[cfg(feature = "boot_probes")] { if boot_probes::HEAP_DEBUG {
        boot_probes::heap_debug_ladder();
    } }

    // Hand off to the desktop compositor event loop.
    // Falls back to scheduler::run_idle_loop if no framebuffer.
    desktop::run()
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    panic::handle(info)
}

