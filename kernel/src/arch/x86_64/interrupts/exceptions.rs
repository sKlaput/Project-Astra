use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use x86_64::structures::idt::{InterruptStackFrame, PageFaultErrorCode};

static RING3_BREAKPOINT_PROBE_ACTIVE: AtomicBool = AtomicBool::new(false);
static RING3_BREAKPOINT_PROBE_HIT: AtomicBool = AtomicBool::new(false);
static RING3_BREAKPOINT_PROBE_RIP: AtomicU64 = AtomicU64::new(0);
static RING3_BREAKPOINT_PROBE_CS: AtomicU64 = AtomicU64::new(0);

pub extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    let code_segment = stack_frame.code_segment.0 as u64;
    if (code_segment & 0x3) == 0x3 {
        let probe_active = RING3_BREAKPOINT_PROBE_ACTIVE.load(Ordering::Relaxed);
        if probe_active {
            RING3_BREAKPOINT_PROBE_ACTIVE.store(false, Ordering::Relaxed);
            RING3_BREAKPOINT_PROBE_HIT.store(true, Ordering::Relaxed);
            RING3_BREAKPOINT_PROBE_RIP
                .store(stack_frame.instruction_pointer.as_u64(), Ordering::Relaxed);
            RING3_BREAKPOINT_PROBE_CS.store(code_segment, Ordering::Relaxed);
        }

        let user_task_breakpoint = crate::scheduler::current_task()
            .map(crate::scheduler::is_user_task)
            .unwrap_or(false);

        if probe_active || user_task_breakpoint {
            let saved_rsp = crate::arch::x86_64::ring3::saved_resume_rsp();
            if saved_rsp != 0 {
                unsafe {
                    crate::arch::x86_64::ring3::resume_saved_stack(saved_rsp);
                }
            }
        }
    }

    crate::serial::write_line("interrupts: breakpoint exception");
}

pub fn arm_ring3_breakpoint_probe() {
    RING3_BREAKPOINT_PROBE_HIT.store(false, Ordering::Relaxed);
    RING3_BREAKPOINT_PROBE_RIP.store(0, Ordering::Relaxed);
    RING3_BREAKPOINT_PROBE_CS.store(0, Ordering::Relaxed);
    RING3_BREAKPOINT_PROBE_ACTIVE.store(true, Ordering::Relaxed);
}

pub fn ring3_breakpoint_probe_hit() -> bool {
    RING3_BREAKPOINT_PROBE_HIT.load(Ordering::Relaxed)
}

pub fn ring3_breakpoint_probe_rip() -> u64 {
    RING3_BREAKPOINT_PROBE_RIP.load(Ordering::Relaxed)
}

pub fn ring3_breakpoint_probe_cs() -> u64 {
    RING3_BREAKPOINT_PROBE_CS.load(Ordering::Relaxed)
}

pub extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    crate::serial::write_line("!!!! DOUBLE FAULT !!!!");
    crate::serial::write_str("RIP: ");
    crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
    crate::serial::write_line("");
    loop {
        core::hint::spin_loop();
    }
}

pub extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) {
    let cpl = stack_frame.code_segment.0 as u64 & 3;

    if cpl == 3 {
        crate::serial::write_str("arch: user-fault gp rip=");
        crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
        crate::serial::write_line("");
        crate::scheduler::abort_current_user_task_from_fault();
    }

    crate::serial::write_line("!!!! GENERAL PROTECTION FAULT !!!!");
    crate::serial::write_str("RIP: ");
    crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
    crate::serial::write_line("");
}

pub extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let cpl = stack_frame.code_segment.0 as u64 & 3;
    let cr2: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }

    let ec = error_code.bits();
    let present = (ec & 0x1) != 0;
    let write = (ec & 0x2) != 0;
    let user = (ec & 0x4) != 0;
    let rsvd = (ec & 0x8) != 0;
    let ifetch = (ec & 0x10) != 0;

    if cpl == 3 {
        crate::serial::write_str("arch: user-fault pf rip=");
        crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
        crate::serial::write_str(" cr2=");
        crate::serial::write_u64(cr2);
        crate::serial::write_str(" ec=");
        crate::serial::write_u64(ec);
        crate::serial::write_str(" cause=");
        crate::serial::write_str(if !present {
            "not-present"
        } else if rsvd {
            "rsvd-pte"
        } else if ifetch {
            "exec-no-x"
        } else if write {
            "write-violation"
        } else {
            "read-violation"
        });
        crate::serial::write_str(if user { " src=user" } else { " src=kernel" });
        crate::serial::write_line("");
        crate::scheduler::abort_current_user_task_from_fault();
    }

    crate::serial::write_line("!!!! PAGE FAULT !!!!");
    crate::serial::write_str("RIP: ");
    crate::serial::write_u64(stack_frame.instruction_pointer.as_u64());
    crate::serial::write_str(" CR2: ");
    crate::serial::write_u64(cr2);
    crate::serial::write_str(" EC: ");
    crate::serial::write_u64(ec);
    crate::serial::write_str(" [");
    if !present {
        crate::serial::write_str("not-present ");
    } else {
        crate::serial::write_str("protection ");
    }
    if write {
        crate::serial::write_str("write ");
    } else {
        crate::serial::write_str("read ");
    }
    if user {
        crate::serial::write_str("user ");
    } else {
        crate::serial::write_str("supervisor ");
    }
    if rsvd {
        crate::serial::write_str("rsvd ");
    }
    if ifetch {
        crate::serial::write_str("ifetch ");
    }
    crate::serial::write_line("]");
}
