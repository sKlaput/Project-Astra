#[unsafe(no_mangle)]
static mut RING3_RESUME_RSP_RAW: u64 = 0;

core::arch::global_asm!(
    ".intel_syntax noprefix",
    "    .global ring3_enter_user_mode",
    "ring3_enter_user_mode:",
    "    mov qword ptr [rip + RING3_RESUME_RSP_RAW], rsp",
    "    push rcx",
    "    push rsi",
    "    push r8",
    "    push rdx",
    "    push rdi",
    "    iretq",
    "    .global ring3_resume_saved_stack",
    "ring3_resume_saved_stack:",
    "    mov rsp, rdi",
    "    ret",
    ".att_syntax prefix",
);

unsafe extern "C" {
    fn ring3_enter_user_mode(entry_ip: u64, user_rsp: u64, user_cs: u64, user_ss: u64, rflags: u64);
    fn ring3_resume_saved_stack(saved_rsp: u64) -> !;
}

pub unsafe fn enter_user_mode(entry_ip: u64, user_rsp: u64, user_cs: u64, user_ss: u64, rflags: u64) {
    unsafe { ring3_enter_user_mode(entry_ip, user_rsp, user_cs, user_ss, rflags) }
}

pub fn saved_resume_rsp() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const RING3_RESUME_RSP_RAW) }
}

pub fn clear_saved_resume_rsp() {
    unsafe {
        core::ptr::write_volatile(&raw mut RING3_RESUME_RSP_RAW, 0);
    }
}

pub unsafe fn resume_saved_stack(saved_rsp: u64) -> ! {
    unsafe { ring3_resume_saved_stack(saved_rsp) }
}