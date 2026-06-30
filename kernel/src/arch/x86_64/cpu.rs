pub fn early_init() {
    enable_fpu_sse();
    enable_kernel_protections();
}

fn enable_fpu_sse() {
    // Safety: early single-core kernel init may configure CR0/CR4 before any
    // floating-point or SIMD state is used.
    unsafe {
        core::arch::asm!(
            "mov rax, cr0",
            "and rax, ~((1 << 2) | (1 << 3))",
            "or rax, (1 << 1) | (1 << 5)",
            "mov cr0, rax",
            "mov rax, cr4",
            "or rax, (1 << 9) | (1 << 10)",
            "mov cr4, rax",
            "fninit",
            out("rax") _,
            options(nostack)
        );
    }
}

// CR0 bits
const CR0_WP: u64 = 1 << 16;
// CR4 bits
const CR4_UMIP: u64 = 1 << 11;
const CR4_SMEP: u64 = 1 << 20;
#[allow(dead_code)]
const CR4_SMAP: u64 = 1 << 21;

use core::sync::atomic::{AtomicBool, Ordering};

static SMAP_ENABLED: AtomicBool = AtomicBool::new(false);

#[inline(always)]
pub fn smap_enabled() -> bool {
    SMAP_ENABLED.load(Ordering::Relaxed)
}

/// Temporarily allow kernel access to user-mapped pages. Wraps the closure
/// in STAC/CLAC if SMAP is active, otherwise calls the closure directly.
#[inline(always)]
pub fn with_user_access<R, F: FnOnce() -> R>(f: F) -> R {
    if SMAP_ENABLED.load(Ordering::Relaxed) {
        unsafe {
            core::arch::asm!("stac", options(nostack, preserves_flags));
        }
        let r = f();
        unsafe {
            core::arch::asm!("clac", options(nostack, preserves_flags));
        }
        r
    } else {
        f()
    }
}

#[derive(Copy, Clone, Default)]
pub struct CpuidLeaf7 {
    pub ebx: u32,
    pub ecx: u32,
    #[allow(dead_code)]
    pub edx: u32,
}

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

fn cpuid_leaf7() -> CpuidLeaf7 {
    let (max_leaf, _, _, _) = cpuid(0, 0);
    if max_leaf < 7 {
        return CpuidLeaf7::default();
    }
    let (_, ebx, ecx, edx) = cpuid(7, 0);
    CpuidLeaf7 { ebx, ecx, edx }
}

pub fn has_smep() -> bool {
    (cpuid_leaf7().ebx & (1 << 7)) != 0
}

pub fn has_smap() -> bool {
    (cpuid_leaf7().ebx & (1 << 20)) != 0
}

pub fn has_umip() -> bool {
    (cpuid_leaf7().ecx & (1 << 2)) != 0
}

pub fn cr0() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mov {0}, cr0", out(reg) v, options(nostack, preserves_flags)) };
    v
}

pub fn cr4() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mov {0}, cr4", out(reg) v, options(nostack, preserves_flags)) };
    v
}

fn enable_kernel_protections() {
    let leaf7 = cpuid_leaf7();
    let smep = (leaf7.ebx & (1 << 7)) != 0;
    let smap = (leaf7.ebx & (1 << 20)) != 0;
    let umip = (leaf7.ecx & (1 << 2)) != 0;

    // CR0.WP — make ring 0 writes respect the read-only PTE bit. Always safe.
    // CR4.SMEP — supervisor-mode execution prevention; ring 0 cannot fetch
    //   from user pages. Safe; kernel never executes user pages directly.
    // CR4.UMIP — block ring 3 from leaking GDTR/IDTR/LDTR/TR/CR0 via SGDT/
    //   SIDT/SLDT/STR/SMSW. Defense-in-depth.
    // CR4.SMAP — supervisor-mode access prevention; ring 0 cannot read/write
    //   user pages unless EFLAGS.AC=1. The kernel uses cpu::with_user_access
    //   to bracket every legitimate user-buffer access with STAC/CLAC.
    unsafe {
        core::arch::asm!(
            "mov rax, cr0",
            "or rax, {wp}",
            "mov cr0, rax",
            wp = const CR0_WP,
            out("rax") _,
            options(nostack),
        );

        let mut cr4_or: u64 = 0;
        if smep {
            cr4_or |= CR4_SMEP;
        }
        if umip {
            cr4_or |= CR4_UMIP;
        }
        if smap {
            cr4_or |= CR4_SMAP;
        }
        if cr4_or != 0 {
            core::arch::asm!(
                "mov rax, cr4",
                "or rax, {bits}",
                "mov cr4, rax",
                bits = in(reg) cr4_or,
                out("rax") _,
                options(nostack),
            );
        }
    }

    if smap {
        // After enabling SMAP, EFLAGS.AC starts at 0; ensure CLAC just in case.
        unsafe {
            core::arch::asm!("clac", options(nostack, preserves_flags));
        }
        SMAP_ENABLED.store(true, Ordering::Relaxed);
    }
}



/// Write to a Model-Specific Register (MSR)
/// 
/// # Safety
/// Must only be used to write to safe MSRs (e.g., GSBASE, FSBASE).
#[inline(always)]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nostack)
        );
    }
}

/// Read from a Model-Specific Register (MSR)
/// 
/// # Safety
/// Must only be used to read safe MSRs (e.g., GSBASE, FSBASE).
#[inline(always)]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nostack)
        );
    }
    ((high as u64) << 32) | (low as u64)
}

/// Set the GS segment base address via GSBASE MSR
/// 
/// # Safety
/// Must be called during per-core initialization before using per-core data.
#[inline(always)]
pub unsafe fn set_gsbase(addr: u64) {
    const GS_BASE_MSR: u32 = 0xC0000101;
    unsafe {
        wrmsr(GS_BASE_MSR, addr);
    }
}

/// Get the current GS segment base address
/// 
/// # Safety
/// Safe to call anytime, but only meaningful after set_gsbase has been called.
#[inline(always)]
pub unsafe fn get_gsbase() -> u64 {
    const GS_BASE_MSR: u32 = 0xC0000101;
    unsafe {
        rdmsr(GS_BASE_MSR)
    }
}