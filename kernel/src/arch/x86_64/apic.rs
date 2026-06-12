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
