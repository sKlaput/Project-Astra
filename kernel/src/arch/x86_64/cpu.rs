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
    let umip = (leaf7.ecx & (1 << 2)) != 0;

    // CR0.WP — make ring 0 writes respect the read-only PTE bit.
    // Always safe to set; usually already on after Limine, but be explicit.
    // CR4.SMEP — supervisor-mode execution prevention; ring 0 cannot fetch
    // from user pages. Safe because the kernel never jumps to user code in
    // ring 0; SYSRET/IRET transition to CPL=3 first.
    // CR4.UMIP — block ring 3 from reading GDTR/IDTR/LDTR/TR/CR0 with
    // SGDT/SIDT/SLDT/STR/SMSW. Defense-in-depth against KASLR-like leaks.
    // CR4.SMAP intentionally left off — the kernel still reads user buffers
    // directly via `from_raw_parts`; enabling SMAP would require STAC/CLAC
    // wrappers around every such access. Tracked separately.
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
        if smep { cr4_or |= CR4_SMEP; }
        if umip { cr4_or |= CR4_UMIP; }
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
}