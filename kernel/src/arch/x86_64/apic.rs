//! Local APIC discovery and CPUID reporting.
//!
//! v0.3 first slice: read-only enumeration. We do **not** yet program the
//! Local APIC timer or bring up application processors — the legacy PIT
//! continues to drive the scheduler. This module exposes:
//!
//! * `vendor_id`, `brand_string`, `family_model_stepping` — CPUID basics.
//! * `lapic_id` — current core's xAPIC ID via CPUID leaf 1 EBX[31:24].
//! * `apic_base_phys` / `apic_global_enable` / `apic_is_bsp` — IA32_APIC_BASE.
//! * `has_x2apic` — CPUID leaf 1 ECX bit 21.
//!
//! No state is mutated; safe to call from any context after `cpu::early_init`.

use crate::boot::protocol;

const CPUID_LEAF_FEATURES: u32 = 1;
const CPUID_LEAF_BRAND_BASE: u32 = 0x8000_0002;
const CPUID_LEAF_EXT_FEATURES: u32 = 0x8000_0001;
const IA32_APIC_BASE_MSR: u32 = 0x1B;

#[inline]
fn cpuid(leaf: u32, sub: u32) -> (u32, u32, u32, u32) {
    let (a, b, c, d): (u32, u32, u32, u32);
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov esi, ebx",
            "pop rbx",
            inout("eax") leaf => a,
            out("esi") b,
            inout("ecx") sub => c,
            out("edx") d,
            options(nostack, preserves_flags),
        );
    }
    (a, b, c, d)
}

#[inline]
fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// 12-byte CPUID vendor ID, NUL-padded to 16 for convenient framing.
pub fn vendor_id() -> [u8; 16] {
    let (_, b, c, d) = cpuid(0, 0);
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&b.to_le_bytes());
    out[4..8].copy_from_slice(&d.to_le_bytes());
    out[8..12].copy_from_slice(&c.to_le_bytes());
    out
}

/// 48-byte CPUID brand string, NUL-padded.
pub fn brand_string() -> [u8; 48] {
    let mut out = [0u8; 48];
    let (max_ext, _, _, _) = cpuid(0x8000_0000, 0);
    if max_ext < CPUID_LEAF_BRAND_BASE + 2 {
        return out;
    }
    for i in 0..3u32 {
        let (a, b, c, d) = cpuid(CPUID_LEAF_BRAND_BASE + i, 0);
        let base = (i as usize) * 16;
        out[base + 0..base + 4].copy_from_slice(&a.to_le_bytes());
        out[base + 4..base + 8].copy_from_slice(&b.to_le_bytes());
        out[base + 8..base + 12].copy_from_slice(&c.to_le_bytes());
        out[base + 12..base + 16].copy_from_slice(&d.to_le_bytes());
    }
    out
}

pub fn family_model_stepping() -> (u32, u32, u32) {
    let (eax, _, _, _) = cpuid(CPUID_LEAF_FEATURES, 0);
    let stepping = eax & 0xF;
    let base_model = (eax >> 4) & 0xF;
    let base_family = (eax >> 8) & 0xF;
    let ext_model = (eax >> 16) & 0xF;
    let ext_family = (eax >> 20) & 0xFF;
    let family = if base_family == 0xF { base_family + ext_family } else { base_family };
    let model = if base_family == 0x6 || base_family == 0xF {
        (ext_model << 4) | base_model
    } else {
        base_model
    };
    (family, model, stepping)
}

/// xAPIC ID of the current core (leaf 1, EBX bits 31:24). Valid when xAPIC
/// is the active mode, which is always true when SMAP/SMEP are on under
/// Limine's default boot.
pub fn lapic_id() -> u32 {
    let (_, ebx, _, _) = cpuid(CPUID_LEAF_FEATURES, 0);
    ebx >> 24
}

pub fn has_x2apic() -> bool {
    let (_, _, ecx, _) = cpuid(CPUID_LEAF_FEATURES, 0);
    (ecx & (1 << 21)) != 0
}

pub fn has_apic() -> bool {
    let (_, _, _, edx) = cpuid(CPUID_LEAF_FEATURES, 0);
    (edx & (1 << 9)) != 0
}

pub fn has_invariant_tsc() -> bool {
    let (max_ext, _, _, _) = cpuid(0x8000_0000, 0);
    if max_ext < 0x8000_0007 {
        return false;
    }
    let (_, _, _, edx) = cpuid(0x8000_0007, 0);
    (edx & (1 << 8)) != 0
}

pub fn has_long_mode() -> bool {
    let (max_ext, _, _, _) = cpuid(0x8000_0000, 0);
    if max_ext < CPUID_LEAF_EXT_FEATURES {
        return false;
    }
    let (_, _, _, edx) = cpuid(CPUID_LEAF_EXT_FEATURES, 0);
    (edx & (1 << 29)) != 0
}

pub struct ApicBase {
    pub raw: u64,
    pub phys: u64,
    pub global_enable: bool,
    pub is_bsp: bool,
    pub x2apic_enable: bool,
}

pub fn read_apic_base() -> ApicBase {
    let raw = rdmsr(IA32_APIC_BASE_MSR);
    ApicBase {
        raw,
        phys: raw & 0x000F_FFFF_FFFF_F000,
        global_enable: (raw & (1 << 11)) != 0,
        is_bsp: (raw & (1 << 8)) != 0,
        x2apic_enable: (raw & (1 << 10)) != 0,
    }
}

/// Reports the multiprocessor topology Limine handed us. Returns
/// `(bsp_lapic_id, total_cpu_count)` on success.
pub fn topology() -> Option<(u32, usize)> {
    let mp = protocol::mp_summary()?;
    Some((mp.bsp_lapic_id, mp.cpu_count))
}

/// Logs a one-line APIC summary on the serial console.
pub fn log_summary() {
    let lid = lapic_id();
    let base = read_apic_base();
    let x2 = has_x2apic();
    let inv = has_invariant_tsc();
    let topo = topology();
    crate::serial::write_str("apic: lapic_id=");
    crate::serial::write_u64(lid as u64);
    crate::serial::write_str(" base=");
    crate::serial::write_u64(base.phys);
    crate::serial::write_str(" enable=");
    crate::serial::write_u64(base.global_enable as u64);
    crate::serial::write_str(" bsp=");
    crate::serial::write_u64(base.is_bsp as u64);
    crate::serial::write_str(" x2apic_supp=");
    crate::serial::write_u64(x2 as u64);
    crate::serial::write_str(" x2apic_on=");
    crate::serial::write_u64(base.x2apic_enable as u64);
    crate::serial::write_str(" invariant_tsc=");
    crate::serial::write_u64(inv as u64);
    crate::serial::write_str(" cpus=");
    crate::serial::write_u64(topo.map(|(_, n)| n as u64).unwrap_or(0));
    crate::serial::write_str(" rsdp=");
    crate::serial::write_u64(protocol::rsdp_address().unwrap_or(0) as u64);
    crate::serial::write_line("");
}


// ──────────────────────────────────────────────────────────────────────────
// Local APIC MMIO + timer calibration.
//
// All accesses go through the HHDM map at phys + paging::hhdm_offset().
// LAPIC MMIO is strongly-ordered uncached on real hardware; under QEMU
// the framebuffer-style HHDM mapping suffices because QEMU emulates the
// LAPIC register file with proper ordering anyway. A future hardening
// pass should remap the LAPIC page with PAT=UC explicitly.
// ──────────────────────────────────────────────────────────────────────────

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering as AOrd};

const LAPIC_REG_ID: usize = 0x020;
const LAPIC_REG_VERSION: usize = 0x030;
#[allow(dead_code)]
const LAPIC_REG_EOI: usize = 0x0B0;
const LAPIC_REG_SVR: usize = 0x0F0;
const LAPIC_REG_LVT_TIMER: usize = 0x320;
const LAPIC_REG_INITIAL_COUNT: usize = 0x380;
const LAPIC_REG_CURRENT_COUNT: usize = 0x390;
const LAPIC_REG_DIVIDE_CONFIG: usize = 0x3E0;

const SVR_ENABLE: u32 = 1 << 8;
const LVT_MASKED: u32 = 1 << 16;

static CALIBRATED: AtomicBool = AtomicBool::new(false);
static TICKS_PER_MS: AtomicU32 = AtomicU32::new(0);
static MEASURED_BUS_HZ: AtomicU32 = AtomicU32::new(0);

#[inline]
fn lapic_virt() -> usize {
    (read_apic_base().phys as usize) + crate::memory::paging::hhdm_offset()
}

#[inline]
unsafe fn lapic_read(reg: usize) -> u32 {
    unsafe { core::ptr::read_volatile((lapic_virt() + reg) as *const u32) }
}

#[inline]
unsafe fn lapic_write(reg: usize, val: u32) {
    unsafe { core::ptr::write_volatile((lapic_virt() + reg) as *mut u32, val) };
}

/// Calibrate the LAPIC timer against the PIT-driven uptime_ms clock.
/// Side effects on the LAPIC: enables SVR with spurious vector 0xFF,
/// programs the timer divider to /16, leaves the timer LVT masked
/// at the end. The scheduler tick source is NOT switched here.
pub fn calibrate_timer() {
    if !read_apic_base().global_enable {
        return;
    }
    if CALIBRATED.load(AOrd::Relaxed) {
        return;
    }

    unsafe {
        let svr = lapic_read(LAPIC_REG_SVR);
        lapic_write(LAPIC_REG_SVR, svr | SVR_ENABLE | 0xFF);
        lapic_write(LAPIC_REG_DIVIDE_CONFIG, 0b0011);
        lapic_write(LAPIC_REG_LVT_TIMER, LVT_MASKED);

        let start_ms = crate::arch::x86_64::interrupts::uptime_ms();
        lapic_write(LAPIC_REG_INITIAL_COUNT, 0xFFFF_FFFF);
        let target = start_ms + 50;
        while crate::arch::x86_64::interrupts::uptime_ms() < target {
            core::hint::spin_loop();
        }
        let end_count = lapic_read(LAPIC_REG_CURRENT_COUNT);
        lapic_write(LAPIC_REG_INITIAL_COUNT, 0);

        let elapsed_ticks = 0xFFFF_FFFFu32.wrapping_sub(end_count);
        let actual_ms = crate::arch::x86_64::interrupts::uptime_ms() - start_ms;
        if actual_ms == 0 {
            return;
        }
        let ticks_per_ms = (elapsed_ticks as u64 / actual_ms) as u32;
        let bus_hz = (ticks_per_ms as u64).saturating_mul(1000).saturating_mul(16) as u32;
        TICKS_PER_MS.store(ticks_per_ms, AOrd::Relaxed);
        MEASURED_BUS_HZ.store(bus_hz, AOrd::Relaxed);
        CALIBRATED.store(true, AOrd::Relaxed);
    }
}

pub fn lapic_ticks_per_ms() -> u32 {
    TICKS_PER_MS.load(AOrd::Relaxed)
}

pub fn lapic_bus_hz() -> u32 {
    MEASURED_BUS_HZ.load(AOrd::Relaxed)
}

pub fn lapic_calibrated() -> bool {
    CALIBRATED.load(AOrd::Relaxed)
}

pub fn lapic_register_id() -> u32 {
    if !read_apic_base().global_enable {
        return u32::MAX;
    }
    unsafe { lapic_read(LAPIC_REG_ID) >> 24 }
}

pub fn lapic_register_version() -> u32 {
    if !read_apic_base().global_enable {
        return 0;
    }
    unsafe { lapic_read(LAPIC_REG_VERSION) }
}

pub fn log_calibration() {
    crate::serial::write_str("apic: lapic_timer cal=");
    crate::serial::write_u64(lapic_calibrated() as u64);
    crate::serial::write_str(" ticks_per_ms=");
    crate::serial::write_u64(lapic_ticks_per_ms() as u64);
    crate::serial::write_str(" bus_hz=");
    crate::serial::write_u64(lapic_bus_hz() as u64);
    crate::serial::write_str(" lapic_id_mmio=");
    crate::serial::write_u64(lapic_register_id() as u64);
    crate::serial::write_str(" lapic_ver=");
    crate::serial::write_u64(lapic_register_version() as u64);
    crate::serial::write_line("");
}
