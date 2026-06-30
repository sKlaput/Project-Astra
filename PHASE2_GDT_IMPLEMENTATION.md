//! Multi-core GDT/TSS allocation for SMP support
//! Replaces the single-core GDT design with per-core capability

use alloc::boxed::Box;
use alloc::vec::Vec;
use spin::Mutex;
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

const DOUBLE_FAULT_STACK_SIZE: usize = 4096;
const PRIVILEGE_STACK_SIZE: usize = 8192;
const KERNEL_STACK_SIZE: usize = 16384;
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

struct PerCoreGdt {
    lapic_id: u32,
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

impl PerCoreGdt {
    fn new(
        lapic_id: u32,
        double_fault_stack: &'static InterruptStack,
        privilege_stack: &'static PrivilegeStack,
        kernel_stack: &'static KernelStack,
    ) -> Self {
        let mut gdt = GlobalDescriptorTable::new();
        let mut tss = TaskStateSegment::new();

        let df_stack_start = VirtAddr::from_ptr(double_fault_stack);
        let df_stack_end = df_stack_start + DOUBLE_FAULT_STACK_SIZE as u64;
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = df_stack_end;

        let priv_stack_start = VirtAddr::from_ptr(privilege_stack);
        let priv_stack_end = priv_stack_start + PRIVILEGE_STACK_SIZE as u64;
        tss.privilege_stack_table[0] = priv_stack_end;

        let kernel_stack_start = VirtAddr::from_ptr(kernel_stack);
        let kernel_stack_end = kernel_stack_start + KERNEL_STACK_SIZE as u64;
        tss.rsp[0] = kernel_stack_end;

        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        let ring3_data_selector = gdt.append(Descriptor::user_data_segment());
        let ring3_code_selector = gdt.append(Descriptor::user_code_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(&tss));

        Self {
            lapic_id,
            _double_fault_stack: double_fault_stack,
            _privilege_stack: privilege_stack,
            _kernel_stack: kernel_stack,
            tss,
            gdt,
            code_selector,
            data_selector,
            tss_selector,
            ring3_code_selector,
            ring3_data_selector,
        }
    }

    unsafe fn load(&self) {
        self.gdt.load();
        CS::set_reg(self.code_selector);
        SS::set_reg(self.data_selector);
        DS::set_reg(self.data_selector);
        ES::set_reg(self.data_selector);
        FS::set_reg(self.data_selector);
        GS::set_reg(self.data_selector);
        load_tss(self.tss_selector);
    }
}

struct PerCoreGdtManager {
    cores: Vec<Option<Box<PerCoreGdt>>>,
    double_fault_stacks: Vec<&'static InterruptStack>,
    privilege_stacks: Vec<&'static PrivilegeStack>,
    kernel_stacks: Vec<&'static KernelStack>,
}

impl PerCoreGdtManager {
    fn new() -> Self {
        Self {
            cores: alloc::vec![None; MAX_CPUS],
            double_fault_stacks: Vec::new(),
            privilege_stacks: Vec::new(),
            kernel_stacks: Vec::new(),
        }
    }

    fn preallocate(&mut self, cpu_count: usize) {
        let count = cpu_count.min(MAX_CPUS);
        for _ in 0..count {
            let df = Box::leak(Box::new(InterruptStack {
                _bytes: [0; DOUBLE_FAULT_STACK_SIZE],
            }));
            let priv = Box::leak(Box::new(PrivilegeStack {
                _bytes: [0; PRIVILEGE_STACK_SIZE],
            }));
            let kern = Box::leak(Box::new(KernelStack {
                _bytes: [0; KERNEL_STACK_SIZE],
            }));
            self.double_fault_stacks.push(df);
            self.privilege_stacks.push(priv);
            self.kernel_stacks.push(kern);
        }
    }

    fn init_core(&mut self, lapic_id: u32) {
        if lapic_id as usize >= MAX_CPUS || self.double_fault_stacks.is_empty() {
            return;
        }

        let df = self.double_fault_stacks.pop().unwrap();
        let priv = self.privilege_stacks.pop().unwrap();
        let kern = self.kernel_stacks.pop().unwrap();

        let core = Box::new(PerCoreGdt::new(lapic_id, df, priv, kern));
        self.cores[lapic_id as usize] = Some(core);
    }

    fn get_core(&self, lapic_id: u32) -> Option<&PerCoreGdt> {
        self.cores
            .get(lapic_id as usize)
            .and_then(|opt| opt.as_ref().map(|b| b.as_ref()))
    }
}

static GDT_MANAGER: Mutex<PerCoreGdtManager> = Mutex::new(PerCoreGdtManager::new());

pub fn init_multicore_gdt(cpu_count: usize) {
    let mut mgr = GDT_MANAGER.lock();
    mgr.preallocate(cpu_count);
    mgr.init_core(0);
    drop(mgr);

    let mgr = GDT_MANAGER.lock();
    if let Some(core) = mgr.get_core(0) {
        unsafe {
            core.load();
        }
        crate::serial::write_line("gdt: per-core BSP GDT loaded");
    }
}

pub fn init_ap_per_core(lapic_id: u32) {
    let mut mgr = GDT_MANAGER.lock();
    mgr.init_core(lapic_id);
    drop(mgr);

    let mgr = GDT_MANAGER.lock();
    if let Some(core) = mgr.get_core(lapic_id) {
        unsafe {
            core.load();
        }
    }
}

pub fn ring3_code_selector() -> SegmentSelector {
    let mgr = GDT_MANAGER.lock();
    mgr.get_core(0)
        .map(|core| core.ring3_code_selector)
        .unwrap_or(SegmentSelector(0))
}

pub fn ring3_data_selector() -> SegmentSelector {
    let mgr = GDT_MANAGER.lock();
    mgr.get_core(0)
        .map(|core| core.ring3_data_selector)
        .unwrap_or(SegmentSelector(0))
}

pub fn kernel_code_selector() -> SegmentSelector {
    let mgr = GDT_MANAGER.lock();
    mgr.get_core(0)
        .map(|core| core.code_selector)
        .unwrap_or(SegmentSelector(0))
}

pub fn kernel_data_selector() -> SegmentSelector {
    let mgr = GDT_MANAGER.lock();
    mgr.get_core(0)
        .map(|core| core.data_selector)
        .unwrap_or(SegmentSelector(0))
}
