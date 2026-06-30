//! Consolidated task metadata table.

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

/// Task state enumeration.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum TaskState {
    Empty = 0,
    Ready = 1,
    Running = 2,
    Sleeping = 3,
}

impl TaskState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => TaskState::Ready,
            2 => TaskState::Running,
            3 => TaskState::Sleeping,
            _ => TaskState::Empty,
        }
    }
}

/// Task ID type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TaskId(pub u64);

/// Single consolidated task metadata entry.
pub struct TaskMetadata {
    pub id: AtomicU64,
    pub state: AtomicU8,
    pub fn_ptr: AtomicU64,
    pub wake_tick: AtomicU64,
    pub enqueue_tick: AtomicU64,
    pub priority: AtomicU8,
    pub slice: AtomicU8,
    pub preempted: AtomicBool,
    pub name_ptr: AtomicU64,
    pub name_len: AtomicU64,
    pub signals: AtomicU64,
    pub signal_mask: AtomicU64,
    pub context_rsp: AtomicU64,
    pub stack_base: AtomicU64,
    pub user_code_virt: AtomicU64,
    pub user_stack_virt: AtomicU64,
    pub user_entry_rip: AtomicU64,
    pub user_rsp: AtomicU64,
    pub user_pml4: AtomicU64,
}

impl TaskMetadata {
    pub const fn new() -> Self {
        Self {
            id: AtomicU64::new(0),
            state: AtomicU8::new(0),
            fn_ptr: AtomicU64::new(0),
            wake_tick: AtomicU64::new(0),
            enqueue_tick: AtomicU64::new(0),
            priority: AtomicU8::new(128),
            slice: AtomicU8::new(5),
            preempted: AtomicBool::new(false),
            name_ptr: AtomicU64::new(0),
            name_len: AtomicU64::new(0),
            signals: AtomicU64::new(0),
            signal_mask: AtomicU64::new(0),
            context_rsp: AtomicU64::new(0),
            stack_base: AtomicU64::new(0),
            user_code_virt: AtomicU64::new(0),
            user_stack_virt: AtomicU64::new(0),
            user_entry_rip: AtomicU64::new(0),
            user_rsp: AtomicU64::new(0),
            user_pml4: AtomicU64::new(0),
        }
    }

    pub fn reset_priority(&self) {
        self.priority.store(128, Ordering::Relaxed);
        self.slice.store(5, Ordering::Relaxed);
    }
}

pub const TABLE_CAP: usize = 16;

/// Consolidated task metadata table.
pub static TASK_TABLE: [TaskMetadata; TABLE_CAP] = [
    TaskMetadata::new(), TaskMetadata::new(), TaskMetadata::new(), TaskMetadata::new(),
    TaskMetadata::new(), TaskMetadata::new(), TaskMetadata::new(), TaskMetadata::new(),
    TaskMetadata::new(), TaskMetadata::new(), TaskMetadata::new(), TaskMetadata::new(),
    TaskMetadata::new(), TaskMetadata::new(), TaskMetadata::new(), TaskMetadata::new(),
];

#[inline]
pub fn table_slot(task_id: u64) -> usize {
    (task_id as usize) % TABLE_CAP
}
