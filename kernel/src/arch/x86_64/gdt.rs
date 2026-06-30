use alloc::boxed::Box;
use spin::Once;
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

const DOUBLE_FAULT_STACK_SIZE: usize = 4096;
const PRIVILEGE_STACK_SIZE: usize = 8192;
const KERNEL_STACK_SIZE: usize = 16384;  // Per-core kernel stack for user transitions
const MAX_CPUS: usize = 256;

#[repr(align(16))]
struct InterruptStack {
    _bytes: [u8; DOUBLE_FAULT_STACK_SIZE],
}

#[repr(align(16))]
struct PrivilegeStack {
    _bytes: [u8; PRIVILEGE_STACK_SIZE],
}

#[repr(align(16))]
struct KernelStack {
    _bytes: [u8; KERNEL_STACK_SIZE],
}

struct GdtState {
    _double_fault_stack: &'static InterruptStack,
    _privilege_stack: &'static PrivilegeStack,
    _kernel_stack: &'static KernelStack,
    tss: TaskStateSegment,
    gdt: GlobalDescriptorTable,
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
    ring3_code_selector: SegmentSelector,
    ring3_data_selector: SegmentSelector,
}

impl GdtState {
    fn new(
        double_fault_stack: &'static InterruptStack,
        privilege_stack: &'static PrivilegeStack,
        kernel_stack: &'static KernelStack,
    ) -> Self {
        Self {
            _double_fault_stack: double_fault_stack,
            _privilege_stack: privilege_stack,
            _kernel_stack: kernel_stack,
            tss: TaskStateSegment::new(),
            gdt: GlobalDescriptorTable::new(),
            code_selector: SegmentSelector(0),
            data_selector: SegmentSelector(0),
            tss_selector: SegmentSelector(0),
            ring3_code_selector: SegmentSelector(0),
            ring3_data_selector: SegmentSelector(0),
        }
    }
}

/// Global GDT state for BSP
static GDT_STATE: Once<&'static GdtState> = Once::new();

/// Per-core GDT states (Phase 2: multicore support)
static PER_CORE_GDTS: Once<[Option<&'static GdtState>; MAX_CPUS]> = Once::new();
static PER_CORE_ALLOC_DONE: core::sync::atomic::AtomicBool = 
    core::sync::atomic::AtomicBool::new(false);

fn gdt_state() -> &'static GdtState {
    GDT_STATE.call_once(|| {
        let double_fault_stack = Box::leak(Box::new(InterruptStack {
            _bytes: [0; DOUBLE_FAULT_STACK_SIZE],
        }));
        let privilege_stack = Box::leak(Box::new(PrivilegeStack {
            _bytes: [0; PRIVILEGE_STACK_SIZE],
        }));
        let kernel_stack = Box::leak(Box::new(KernelStack {
            _bytes: [0; KERNEL_STACK_SIZE],
        }));
        let state: &'static mut GdtState =
            Box::leak(Box::new(GdtState::new(double_fault_stack, privilege_stack, kernel_stack)));
        
        let stack_start = VirtAddr::from_ptr(state._double_fault_stack);
        let stack_end = stack_start + DOUBLE_FAULT_STACK_SIZE as u64;
        let privilege_stack_start = VirtAddr::from_ptr(state._privilege_stack);
        let privilege_stack_end = privilege_stack_start + PRIVILEGE_STACK_SIZE as u64;
        
        state.tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_end;
        state.tss.privilege_stack_table[0] = privilege_stack_end;

        state.code_selector = state.gdt.append(Descriptor::kernel_code_segment());
        state.data_selector = state.gdt.append(Descriptor::kernel_data_segment());
        state.ring3_data_selector = state.gdt.append(Descriptor::user_data_segment());
        state.ring3_code_selector = state.gdt.append(Descriptor::user_code_segment());
        state.tss_selector = state.gdt.append(Descriptor::tss_segment(&state.tss));

        state
    })
}

/// Initialize per-core GDT for a specific LAPIC ID (called during AP startup)
fn alloc_gdt_for_lapic(lapic_id: u32) -> &'static GdtState {
    let double_fault_stack = Box::leak(Box::new(InterruptStack {
        _bytes: [0; DOUBLE_FAULT_STACK_SIZE],
    }));
    let privilege_stack = Box::leak(Box::new(PrivilegeStack {
        _bytes: [0; PRIVILEGE_STACK_SIZE],
    }));
    let kernel_stack = Box::leak(Box::new(KernelStack {
        _bytes: [0; KERNEL_STACK_SIZE],
    }));
    let state: &'static mut GdtState =
        Box::leak(Box::new(GdtState::new(double_fault_stack, privilege_stack, kernel_stack)));
    
    let stack_start = VirtAddr::from_ptr(state._double_fault_stack);
    let stack_end = stack_start + DOUBLE_FAULT_STACK_SIZE as u64;
    let privilege_stack_start = VirtAddr::from_ptr(state._privilege_stack);
    let privilege_stack_end = privilege_stack_start + PRIVILEGE_STACK_SIZE as u64;
    
    state.tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_end;
    state.tss.privilege_stack_table[0] = privilege_stack_end;

    state.code_selector = state.gdt.append(Descriptor::kernel_code_segment());
    state.data_selector = state.gdt.append(Descriptor::kernel_data_segment());
    state.ring3_data_selector = state.gdt.append(Descriptor::user_data_segment());
    state.ring3_code_selector = state.gdt.append(Descriptor::user_code_segment());
    state.tss_selector = state.gdt.append(Descriptor::tss_segment(&state.tss));

    state
}

/// Install a kernel-owned GDT for the BSP
pub fn init() {
    let state = gdt_state();

    unsafe {
        state.gdt.load();
        CS::set_reg(state.code_selector);
        SS::set_reg(state.data_selector);
        DS::set_reg(state.data_selector);
        ES::set_reg(state.data_selector);
        FS::set_reg(state.data_selector);
        GS::set_reg(state.data_selector);
        load_tss(state.tss_selector);
    }

    crate::serial::write_line("gdt: kernel GDT + TSS + ring-3 descriptors active");
}

/// Phase 2: Initialize multicore GDT system (called from smp::init)
pub fn init_multicore_gdt(cpu_count: usize) {
    crate::serial::write_str("gdt: multicore initialization for ");
    crate::serial::write_u64(cpu_count as u64);
    crate::serial::write_line(" CPUs");
    
    PER_CORE_ALLOC_DONE.store(true, core::sync::atomic::Ordering::Release);
}

/// Load per-core GDT for an Application Processor
pub fn init_ap_per_core(lapic_id: u32) {
    let state = alloc_gdt_for_lapic(lapic_id);

    unsafe {
        state.gdt.load();
        CS::set_reg(state.code_selector);
        SS::set_reg(state.data_selector);
        DS::set_reg(state.data_selector);
        ES::set_reg(state.data_selector);
        FS::set_reg(state.data_selector);
        GS::set_reg(state.data_selector);
        load_tss(state.tss_selector);
    }

    crate::serial::write_str("gdt: per-core AP GDT loaded lapic=");
    crate::serial::write_u32(lapic_id);
    crate::serial::write_line("");
}

/// Backward compat wrapper for init_ap
pub fn init_ap() {
    init_ap_per_core(0);
}

/// Get ring-3 code selector
pub fn ring3_code_selector() -> SegmentSelector {
    gdt_state().ring3_code_selector
}

/// Get ring-3 data selector
pub fn ring3_data_selector() -> SegmentSelector {
    gdt_state().ring3_data_selector
}

/// Get kernel code selector
pub fn kernel_code_selector() -> SegmentSelector {
    gdt_state().code_selector
}

/// Get kernel data selector
pub fn kernel_data_selector() -> SegmentSelector {
    gdt_state().data_selector
}