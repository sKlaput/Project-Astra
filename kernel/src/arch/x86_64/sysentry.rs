use core::arch::asm;

const IA32_EFER: u32 = 0xC000_0080;
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;
const EFER_SCE: u64 = 1;
const RFLAGS_IF: u64 = 1 << 9;

const SYSCALL_STACK_SIZE: usize = 32 * 1024;

#[repr(C, align(16))]
struct SyscallKernelStack {
    bytes: [u8; SYSCALL_STACK_SIZE],
}

/// Scratch cell: user RSP saved here on every SYSCALL entry, restored before SYSRETQ.
/// Single-core-only; would become per-CPU in an SMP kernel.
#[unsafe(no_mangle)]
static mut SYSCALL_USER_RSP: u64 = 0;

/// Dedicated syscall kernel stack storage.
/// Kept as a fixed static region so entry does not depend on mutable stack-top state.
#[unsafe(no_mangle)]
static mut SYSCALL_KERNEL_STACK: SyscallKernelStack = SyscallKernelStack {
    bytes: [0u8; SYSCALL_STACK_SIZE],
};

// ---------------------------------------------------------------------------
// Real SYSCALL entry stub.
//
// Stack contract at entry (SYSCALL instruction has already run):
//   RCX  = user return RIP
//   R11  = user RFLAGS (IF was cleared because FMASK has bit 9 set)
//   RSP  = user stack (CPU does NOT switch stacks on SYSCALL)
//   CS   = kernel code selector, SS = kernel data selector
//
// We save the user RSP, switch to a dedicated kernel stack, save
// callee-saved registers, shuffle the syscall arguments into the
// SysV AMD64 calling convention, call syscall_dispatch_rust, and
// then restore everything and sysretq.
//
// Stack layout during dispatch (offsets from SYSCALL_KERNEL_RSP_TOP):
//   -8  rcx (user RIP)
//   -16 r11 (user RFLAGS)
//   -24 rbp
//   -32 rbx
//   -40 r12
//   -48 r13
//   -56 r14
//   -64 r15          <- 0 mod 16 after 8 pushes from 16-byte-aligned top
//   -72 rax          <- alignment pad (RSP now 8 mod 16)
//   -80 r9 (a6)      <- 7th arg on stack (RSP now 0 mod 16 before call)
//   -88 return addr  <- pushed by `call` (8 mod 16 at function entry)
//   At function entry [RSP+8] = a6, satisfying SysV stack-arg convention.
// ---------------------------------------------------------------------------
core::arch::global_asm!(
    ".intel_syntax noprefix",
    "    .global syscall_entry_stub",
    "syscall_entry_stub:",

    // Save user RSP and switch to dedicated kernel syscall stack.
    "    mov qword ptr [rip + SYSCALL_USER_RSP], rsp",
    "    lea rsp, [rip + SYSCALL_KERNEL_STACK + 32768]",

    // Save user return state and callee-saved registers (8 pushes => 0 mod 16).
    "    push rcx",
    "    push r11",
    "    push rbp",
    "    push rbx",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",

    // Alignment dummy + 7th syscall arg.
    // push rax => RSP at 8 mod 16 (also preserves nr for the shuffle).
    // push r9  => RSP at 0 mod 16 (7th arg in correct pre-call position).
    "    push rax",
    "    push r9",

    // Shuffle syscall ABI -> SysV ABI for dispatch(nr, a1..a6):
    //   syscall: rax=nr, rdi=a1, rsi=a2, rdx=a3, r10=a4, r8=a5, r9=a6 (pushed)
    //   SysV:    rdi,   rsi,    rdx,    rcx,    r8,    r9,    [rsp+8 at entry]
    "    mov r9,  r8",
    "    mov r8,  r10",
    "    mov rcx, rdx",
    "    mov rdx, rsi",
    "    mov rsi, rdi",
    "    mov rdi, rax",

    "    call syscall_dispatch_rust",

    // Return value is in RAX.  Clean up 7th-arg push + alignment dummy.
    "    add rsp, 16",

    // Restore callee-saved registers.
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbx",
    "    pop rbp",
    "    pop r11",
    "    pop rcx",

    // Restore user RSP and return to CPL3.
    "    mov rsp, qword ptr [rip + SYSCALL_USER_RSP]",
    "    sysretq",
    ".att_syntax prefix",
);

unsafe extern "C" {
    fn syscall_entry_stub();
}

/// Bridge called by the SYSCALL stub via SysV ABI; forwards to the kernel
/// dispatch table and returns the result in RAX.
#[unsafe(no_mangle)]
pub extern "C" fn syscall_dispatch_rust(
    nr: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
) -> u64 {
    crate::syscall::dispatch(nr, a1, a2, a3, a4, a5, a6)
}

fn read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | (low as u64)
}

fn write_msr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub fn init() {
    let kernel_cs = crate::arch::x86_64::gdt::kernel_code_selector().0 as u64;
    let user_data = crate::arch::x86_64::gdt::ring3_data_selector().0 as u64;
    let user_star_base = user_data.saturating_sub(8);
    let star = (user_star_base << 48) | (kernel_cs << 32);

    let efer = read_msr(IA32_EFER);
    write_msr(IA32_EFER, efer | EFER_SCE);
    write_msr(IA32_STAR, star);
    write_msr(IA32_LSTAR, syscall_entry_addr());
    // Clear IF on SYSCALL entry so the 2-instruction window between
    // "save user RSP" and "load kernel RSP" cannot be interrupted.
    write_msr(IA32_FMASK, RFLAGS_IF);

    crate::serial::write_line("arch: syscall/sysret MSRs configured");
}

pub fn efer() -> u64 {
    read_msr(IA32_EFER)
}

pub fn star() -> u64 {
    read_msr(IA32_STAR)
}

pub fn lstar() -> u64 {
    read_msr(IA32_LSTAR)
}

pub fn fmask() -> u64 {
    read_msr(IA32_FMASK)
}

pub fn syscall_entry_addr() -> u64 {
    syscall_entry_stub as usize as u64
}