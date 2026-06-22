use crate::{arch, console, memory, serial};

/// Set to true to run the full heap debug ladder + churn test at boot.
/// Leave false for normal clean boots.
pub(crate) const HEAP_DEBUG: bool = false;

/// When HEAP_DEBUG is true, halt execution after this ladder step.
/// None = run all steps without halting.
pub(crate) const HEAP_DEBUG_HALT_AFTER_STEP: Option<u8> = None;

/// Set true to force one allocator failure and validate alloc-error diagnostics.
/// This probe is expected to halt in the alloc error handler.
pub(crate) const HEAP_ALLOC_FAILURE_PROBE: bool = false;

pub(crate) fn probe_alloc_failure_path() {
    use alloc::vec::Vec;

    serial::write_line("heap: alloc-failure probe armed");
    memory::heap::inject_alloc_failures(1);

    let mut trigger: Vec<u8> = Vec::with_capacity(64);
    trigger.push(0xAA);

    serial::write_line("heap: alloc-failure probe did not trigger");
}

pub(crate) fn heap_debug_ladder() {
    use alloc::alloc::alloc;
    use alloc::boxed::Box;
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::alloc::Layout;

    console::log("heap: deterministic test ladder start");

    // [HEAP-1] raw alloc
    let layout_small = Layout::from_size_align(32, 8).unwrap();
    let layout_aligned = Layout::from_size_align(128, 64).unwrap();
    let ptr_small = unsafe { alloc(layout_small) };
    let ptr_aligned = unsafe { alloc(layout_aligned) };
    if !ptr_small.is_null() && !ptr_aligned.is_null() {
        console::log("[HEAP-1] raw alloc OK");
        heap_debug_maybe_halt(1);
    } else {
        console::log("[HEAP-1] raw alloc FAIL");
        arch::x86_64::halt::halt_loop();
    }

    // [HEAP-2] Box
    let boxed = Box::new(0xC0FFEE_u64);
    if *boxed == 0xC0FFEE_u64 {
        console::log("[HEAP-2] Box OK");
        heap_debug_maybe_halt(2);
    } else {
        console::log("[HEAP-2] Box FAIL");
        arch::x86_64::halt::halt_loop();
    }

    // [HEAP-3] Vec
    let mut values: Vec<u32> = Vec::with_capacity(4);
    values.push(10);
    values.push(20);
    values.push(30);
    values.push(40);
    if values.len() == 4 && values[3] == 40 {
        console::log("[HEAP-3] Vec OK");
        heap_debug_maybe_halt(3);
    } else {
        console::log("[HEAP-3] Vec FAIL");
        arch::x86_64::halt::halt_loop();
    }

    // [HEAP-4] String
    let mut text = String::from("heap");
    text.push_str("-ok");
    if text.as_str() == "heap-ok" {
        console::log("[HEAP-4] String OK");
        heap_debug_maybe_halt(4);
    } else {
        console::log("[HEAP-4] String FAIL");
        arch::x86_64::halt::halt_loop();
    }

    // [HEAP-5] allocation churn: 200 small Box allocations
    let mut churn_ok = true;
    for i in 0_u64..200 {
        let b = Box::new(i);
        if *b != i {
            churn_ok = false;
            break;
        }
    }
    if churn_ok {
        console::log("[HEAP-5] churn 200x Box OK");
        heap_debug_maybe_halt(5);
    } else {
        console::log("[HEAP-5] churn FAIL");
        arch::x86_64::halt::halt_loop();
    }

    memory::heap::report_heap_status();
    console::log("heap: debug ladder complete");
}

fn heap_debug_maybe_halt(step: u8) {
    if HEAP_DEBUG_HALT_AFTER_STEP == Some(step) {
        console::log("heap: temporary halt for ladder isolation");
        arch::x86_64::halt::halt_loop();
    }
}
