pub fn early_init() {
    enable_fpu_sse();
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