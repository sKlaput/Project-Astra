use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcessState {
    Empty = 0,
    Running = 1,
    Exited = 2,
}

/// User-space startup ABI version contract.
/// 
/// Version 1 (current):
/// - Entry: ring-3 code at 0x400000 (ELF p_vaddr)
/// - Stack: user-allocated at 0x500000+ (provided at spawn time)
/// - Syscalls: rax=number, args in rdi/rsi/rdx/r10/r8/r9 (SysV AMD64 convention)
/// - Return: int3 from ring-3 to signal completion back to kernel
#[derive(Debug, Clone, Copy)]
pub struct UserStartupAbi {
    pub version: u32,
    /// Code virtual base (typically 0x400000 for ELF)
    pub code_virt: u64,
    /// Stack virtual base (typically 0x500000+)
    pub stack_virt: u64,
}

impl UserStartupAbi {
    /// Construct ABI v1 with standard virtual address layout.
    pub fn v1() -> Self {
        Self {
            version: 1,
            code_virt: 0x400000,
            stack_virt: 0x500000,
        }
    }

    /// Validate that this ABI version is supported.
    pub fn is_supported(&self) -> bool {
        self.version == PROCESS_STARTUP_ABI_VERSION
    }
}

#[derive(Clone, Copy)]
struct ProcessEntry {
    pid: u64,
    main_task: u64,
    state: ProcessState,
    start_tick: u64,
    exit_tick: u64,
    startup_abi_version: u32,
    name_ptr: u64,
    name_len: u64,
}

impl ProcessEntry {
    const fn empty() -> Self {
        Self {
            pid: 0,
            main_task: 0,
            state: ProcessState::Empty,
            start_tick: 0,
            exit_tick: 0,
            startup_abi_version: 0,
            name_ptr: 0,
            name_len: 0,
        }
    }
}

pub const TABLE_CAP: usize = 16;
const PROCESS_STARTUP_ABI_VERSION: u32 = 1;

static NEXT_PID: AtomicU64 = AtomicU64::new(1);
static PROCESS_TABLE: Mutex<[ProcessEntry; TABLE_CAP]> = Mutex::new([ProcessEntry::empty(); TABLE_CAP]);

pub fn startup_abi_version() -> u32 {
    PROCESS_STARTUP_ABI_VERSION
}

/// Validate that an ABI contract is satisfied.
/// Returns true if the ABI version matches the kernel's supported version.
pub fn validate_startup_abi(abi: &UserStartupAbi) -> bool {
    abi.is_supported()
}

fn register_process(name: &'static str, task_id: crate::scheduler::TaskId) -> Option<ProcessId> {
    let pid = ProcessId(NEXT_PID.fetch_add(1, Ordering::Relaxed));
    let now = crate::scheduler::ticks();

    let mut table = PROCESS_TABLE.lock();
    let slot = find_reusable_slot(&table)?;
    table[slot] = ProcessEntry {
        pid: pid.0,
        main_task: task_id.0,
        state: ProcessState::Running,
        start_tick: now,
        exit_tick: 0,
        startup_abi_version: PROCESS_STARTUP_ABI_VERSION,
        name_ptr: name.as_ptr() as u64,
        name_len: name.len() as u64,
    };

    Some(pid)
}

fn find_slot_by_pid(table: &[ProcessEntry; TABLE_CAP], pid: ProcessId) -> Option<usize> {
    for (idx, entry) in table.iter().enumerate() {
        if entry.pid == pid.0 {
            return Some(idx);
        }
    }
    None
}

fn find_reusable_slot(table: &[ProcessEntry; TABLE_CAP]) -> Option<usize> {
    for (idx, entry) in table.iter().enumerate() {
        if entry.state == ProcessState::Empty || entry.state == ProcessState::Exited {
            return Some(idx);
        }
    }
    None
}

fn refresh_entry_state(entry: &mut ProcessEntry) -> ProcessState {
    let task_id = crate::scheduler::TaskId(entry.main_task);
    let scheduler_state = crate::scheduler::task_state(task_id);

    if scheduler_state == crate::scheduler::TaskState::Empty {
        // Task is gone; reclaim its user address-space page-table structures.
        if let Some(pml4) = crate::scheduler::take_task_user_pml4(task_id) {
            crate::memory::paging::destroy_user_space_root(pml4 as usize);
        }
        entry.state = ProcessState::Exited;
        if entry.exit_tick == 0 {
            entry.exit_tick = crate::scheduler::ticks();
        }
    } else if entry.state != ProcessState::Empty {
        entry.state = ProcessState::Running;
    }

    entry.state
}

pub fn spawn_elf_process(
    name: &'static str,
    elf_image: &'static [u8],
    user_stack_virt: usize,
    priority: u8,
) -> Option<ProcessId> {
    // Validate ABI constraints before spawning.
    let abi = UserStartupAbi::v1();
    if !validate_startup_abi(&abi) {
        return None;
    }

    let user_pml4_phys = crate::memory::paging::clone_kernel_space_root()?;

    let entry_rip = crate::loader::load_elf_into_pml4(elf_image, user_pml4_phys).ok()?;

    if user_stack_virt % crate::memory::paging::PAGE_SIZE != 0 {
        return None;
    }
    if !crate::memory::paging::is_user_range(user_stack_virt, crate::memory::paging::PAGE_SIZE) {
        return None;
    }

    let frame = crate::memory::frame_allocator::allocate_frame()?;
    let flags = crate::memory::paging::PageTableFlags::new(
        crate::memory::paging::PageTableFlags::PRESENT
            | crate::memory::paging::PageTableFlags::WRITABLE
            | crate::memory::paging::PageTableFlags::USER_ACCESSIBLE,
    );
    unsafe {
        crate::memory::paging::map_page_in_pml4(user_pml4_phys, user_stack_virt, frame.start_address(), flags).ok()?;
    }

    let user_rsp = user_stack_virt as u64 + crate::memory::paging::PAGE_SIZE as u64 - 8;
    let task_id = crate::scheduler::spawn_user_task_prio_name(
        0x400000,
        user_stack_virt as u64,
        entry_rip,
        user_rsp,
        priority,
        name,
    )?;

    if !crate::scheduler::set_task_user_pml4(task_id, user_pml4_phys as u64) {
        crate::scheduler::exit_task(task_id);
        return None;
    }

    register_process(name, task_id)
}

pub fn spawn_kernel_process(name: &'static str, f: fn(), priority: u8) -> Option<ProcessId> {
    let task_id = crate::scheduler::spawn_task_with_fn_prio_name(f, priority, name)?;
    register_process(name, task_id)
}

pub fn refresh_state(pid: ProcessId) -> Option<ProcessState> {
    let mut table = PROCESS_TABLE.lock();
    let slot = find_slot_by_pid(&table, pid)?;
    Some(refresh_entry_state(&mut table[slot]))
}

pub fn state(pid: ProcessId) -> Option<ProcessState> {
    refresh_state(pid)
}

pub fn main_task(pid: ProcessId) -> Option<crate::scheduler::TaskId> {
    let table = PROCESS_TABLE.lock();
    let slot = find_slot_by_pid(&table, pid)?;
    Some(crate::scheduler::TaskId(table[slot].main_task))
}

pub fn process_name_len(pid: ProcessId) -> Option<u64> {
    let table = PROCESS_TABLE.lock();
    let slot = find_slot_by_pid(&table, pid)?;
    Some(table[slot].name_len)
}

pub fn startup_version(pid: ProcessId) -> Option<u32> {
    let table = PROCESS_TABLE.lock();
    let slot = find_slot_by_pid(&table, pid)?;
    Some(table[slot].startup_abi_version)
}

pub fn uptime_ticks(pid: ProcessId) -> Option<u64> {
    let mut table = PROCESS_TABLE.lock();
    let slot = find_slot_by_pid(&table, pid)?;
    let entry = &mut table[slot];
    let state = refresh_entry_state(entry);
    let end_tick = if state == ProcessState::Exited {
        entry.exit_tick
    } else {
        crate::scheduler::ticks()
    };
    Some(end_tick.saturating_sub(entry.start_tick))
}

/// Snapshot of a single process entry for display purposes.
#[derive(Clone, Copy)]
pub struct ProcessInfo {
    pub pid:        u64,
    pub state:      ProcessState,
    pub task_id:    u64,
    pub start_tick: u64,
    /// Up to 16 bytes of the process name (UTF-8).
    pub name:       [u8; 16],
    pub name_len:   usize,
}

impl ProcessInfo {
    const fn empty() -> Self {
        Self {
            pid: 0,
            state: ProcessState::Empty,
            task_id: 0,
            start_tick: 0,
            name: [0u8; 16],
            name_len: 0,
        }
    }
}

/// Returns a snapshot of all non-empty process table entries and the count.
pub fn list_all() -> ([ProcessInfo; TABLE_CAP], usize) {
    let mut table = PROCESS_TABLE.lock();
    let mut out = [ProcessInfo::empty(); TABLE_CAP];
    let mut count = 0usize;
    for entry in table.iter_mut() {
        if entry.state == ProcessState::Empty { continue; }
        refresh_entry_state(entry);
        let name_ptr = entry.name_ptr as *const u8;
        let name_len = (entry.name_len as usize).min(16);
        let mut name = [0u8; 16];
        if !name_ptr.is_null() && name_len > 0 {
            // SAFETY: name_ptr was set from a &'static str at spawn time.
            unsafe {
                let src = core::slice::from_raw_parts(name_ptr, name_len);
                name[..name_len].copy_from_slice(src);
            }
        }
        out[count] = ProcessInfo {
            pid: entry.pid,
            state: entry.state,
            task_id: entry.main_task,
            start_tick: entry.start_tick,
            name,
            name_len,
        };
        count += 1;
    }
    (out, count)
}

/// Count currently running (non-exited) user processes.
pub fn count_running_user() -> usize {
    let mut table = PROCESS_TABLE.lock();
    let mut count = 0usize;
    for entry in table.iter_mut() {
        if entry.state == ProcessState::Empty { continue; }
        let state = refresh_entry_state(entry);
        if state == ProcessState::Running {
            // Only count entries that correspond to a user task.
            let tid = crate::scheduler::TaskId(entry.main_task);
            if crate::scheduler::is_user_task(tid) {
                count += 1;
            }
        }
    }
    count
}

/// Returns counts for (running, exited, empty) entries in the process table.
pub fn state_counts() -> (usize, usize, usize) {
    let mut table = PROCESS_TABLE.lock();
    let mut running = 0usize;
    let mut exited = 0usize;
    let mut empty = 0usize;

    for entry in table.iter_mut() {
        let state = refresh_entry_state(entry);
        match state {
            ProcessState::Running => running += 1,
            ProcessState::Exited => exited += 1,
            ProcessState::Empty => empty += 1,
        }
    }

    (running, exited, empty)
}
