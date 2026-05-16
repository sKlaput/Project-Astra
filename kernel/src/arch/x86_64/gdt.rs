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

#[repr(align(16))]
struct InterruptStack {
    _bytes: [u8; DOUBLE_FAULT_STACK_SIZE],
}

#[repr(align(16))]
struct PrivilegeStack {
    _bytes: [u8; PRIVILEGE_STACK_SIZE],
}

struct GdtState {
    _double_fault_stack: &'static InterruptStack,
    _privilege_stack: &'static PrivilegeStack,
    tss: TaskStateSegment,
    gdt: GlobalDescriptorTable,
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
    ring3_code_selector: SegmentSelector,
    ring3_data_selector: SegmentSelector,
}

impl GdtState {
    fn new(double_fault_stack: &'static InterruptStack, privilege_stack: &'static PrivilegeStack) -> Self {
        Self {
            _double_fault_stack: double_fault_stack,
            _privilege_stack: privilege_stack,
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

static GDT_STATE: Once<&'static GdtState> = Once::new();

fn gdt_state() -> &'static GdtState {
    GDT_STATE.call_once(|| {
        let double_fault_stack = Box::leak(Box::new(InterruptStack {
            _bytes: [0; DOUBLE_FAULT_STACK_SIZE],
        }));
        let privilege_stack = Box::leak(Box::new(PrivilegeStack {
            _bytes: [0; PRIVILEGE_STACK_SIZE],
        }));
        let state: &'static mut GdtState = Box::leak(Box::new(GdtState::new(double_fault_stack, privilege_stack)));
        let stack_start = VirtAddr::from_ptr(state._double_fault_stack);
        let stack_end = stack_start + DOUBLE_FAULT_STACK_SIZE as u64;
        let privilege_stack_start = VirtAddr::from_ptr(state._privilege_stack);
        let privilege_stack_end = privilege_stack_start + PRIVILEGE_STACK_SIZE as u64;
        state.tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_end;
        state.tss.privilege_stack_table[0] = privilege_stack_end;

        state.code_selector = state.gdt.append(Descriptor::kernel_code_segment());
        state.data_selector = state.gdt.append(Descriptor::kernel_data_segment());
        // Keep user data immediately before user code so SYSRET selector math
        // works: STAR[63:48] + 8 = user data, +16 = user code.
        state.ring3_data_selector = state.gdt.append(Descriptor::user_data_segment());
        state.ring3_code_selector = state.gdt.append(Descriptor::user_code_segment());
        state.tss_selector = state.gdt.append(Descriptor::tss_segment(&state.tss));

        state
    })
}

/// Install a kernel-owned GDT so interrupt delivery can look up the
/// code-segment descriptor without faulting on Limine's unmapped GDT page.
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

/// Get ring-3 code selector for user-space tasks.
pub fn ring3_code_selector() -> SegmentSelector {
    gdt_state().ring3_code_selector
}

/// Get ring-3 data selector for user-space tasks.
pub fn ring3_data_selector() -> SegmentSelector {
    gdt_state().ring3_data_selector
}

/// Get kernel code selector.
pub fn kernel_code_selector() -> SegmentSelector {
    gdt_state().code_selector
}

/// Get kernel data selector.
pub fn kernel_data_selector() -> SegmentSelector {
    gdt_state().data_selector
}
