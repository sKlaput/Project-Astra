use alloc::boxed::Box;
use spin::Once;
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

use crate::arch::x86_64::percpu::PerCpuData;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

const DOUBLE_FAULT_STACK_SIZE: usize = 4096;
const PRIVILEGE_STACK_SIZE: usize = 8192;
const KERNEL_STACK_SIZE: usize = 16384;

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
    _tss: &'static TaskStateSegment,
    gdt: GlobalDescriptorTable,
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
    ring3_code_selector: SegmentSelector,
    ring3_data_selector: SegmentSelector,
    percpu_data: &'static mut PerCpuData,
}

impl GdtState {
    fn new(
        double_fault_stack: &'static InterruptStack,
        privilege_stack: &'static PrivilegeStack,
        kernel_stack: &'static KernelStack,
        tss: &'static TaskStateSegment,
        lapic_id: u32,
    ) -> Self {
        let mut gdt = GlobalDescriptorTable::new();

        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        let ring3_data_selector = gdt.append(Descriptor::user_data_segment());
        let ring3_code_selector = gdt.append(Descriptor::user_code_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(tss));

        // Allocate per-core data structure
        let percpu_data = PerCpuData::new(lapic_id);

        Self {
            _double_fault_stack: double_fault_stack,
            _privilege_stack: privilege_stack,
            _kernel_stack: kernel_stack,
            _tss: tss,
            gdt,
            code_selector,
            data_selector,
            tss_selector,
            ring3_code_selector,
            ring3_data_selector,
            percpu_data,
        }
    }
}

/// Create and initialize a TSS with the given stacks
fn init_tss(
    double_fault_stack: &InterruptStack,
    privilege_stack: &PrivilegeStack,
) -> &'static TaskStateSegment {
    let mut tss = Box::new(TaskStateSegment::new());
    
    let df_stack_start = VirtAddr::from_ptr(double_fault_stack);
    let df_stack_end = df_stack_start + DOUBLE_FAULT_STACK_SIZE as u64;
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = df_stack_end;

    let priv_stack_start = VirtAddr::from_ptr(privilege_stack);
    let priv_stack_end = priv_stack_start + PRIVILEGE_STACK_SIZE as u64;
    tss.privilege_stack_table[0] = priv_stack_end;
    
    Box::leak(tss)
}

static GDT_STATE: Once<&'static GdtState> = Once::new();

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
        let tss = init_tss(double_fault_stack, privilege_stack);
        
        let state = GdtState::new(double_fault_stack, privilege_stack, kernel_stack, tss, 0);
        let state_ptr = Box::leak(Box::new(state));
        unsafe { crate::arch::x86_64::percpu::register_percpu_data(0, state_ptr.percpu_data as *mut _) };
        state_ptr
    })
}

/// Initialize per-core GDT and allocate per-core data
fn alloc_gdt_for_lapic(lapic_id: u32) -> (&'static GdtState, u64) {
    let double_fault_stack = Box::leak(Box::new(InterruptStack {
        _bytes: [0; DOUBLE_FAULT_STACK_SIZE],
    }));
    let privilege_stack = Box::leak(Box::new(PrivilegeStack {
        _bytes: [0; PRIVILEGE_STACK_SIZE],
    }));
    let kernel_stack = Box::leak(Box::new(KernelStack {
        _bytes: [0; KERNEL_STACK_SIZE],
    }));
    let tss = init_tss(double_fault_stack, privilege_stack);
    
    let state = GdtState::new(double_fault_stack, privilege_stack, kernel_stack, tss, lapic_id);
    let gsbase_addr = state.percpu_data as *const _ as u64;
    let state_ptr = Box::leak(Box::new(state));
    
    (state_ptr, gsbase_addr)
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
        
        // Set GSBASE for BSP per-core data
        crate::arch::x86_64::cpu::set_gsbase(state.percpu_data as *const _ as u64);
    }

    crate::serial::write_line("gdt: kernel GDT + TSS + ring-3 descriptors active");
    crate::serial::write_line("percpu: BSP per-core data initialized cpu_id=0");
}

/// Phase 2: Initialize multicore GDT system (called from smp::init)
pub fn init_multicore_gdt(cpu_count: usize) {
    crate::serial::write_str("gdt: multicore initialization for ");
    crate::serial::write_u64(cpu_count as u64);
    crate::serial::write_line(" CPUs");
}

/// Load per-core GDT for an Application Processor
/// Returns the GSBASE address that should be set
pub fn init_ap_per_core(lapic_id: u32) -> u64 {
    let (state, gsbase_addr) = alloc_gdt_for_lapic(lapic_id);

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

    gsbase_addr
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