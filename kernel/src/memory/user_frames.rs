// ---------------------------------------------------------------------------
// Astra OS — Per-process owned user-leaf frame tracking.
//
// Records the physical frames allocated specifically for a user process
// (ELF code/data pages, user stack, etc.) so they can be reclaimed when the
// process exits without freeing shared kernel-managed mappings.
// ---------------------------------------------------------------------------

use spin::Mutex;
use crate::memory::frame_allocator::{deallocate_frame, Frame};

const MAX_PROCS: usize = 16;
const MAX_FRAMES_PER_PROC: usize = 256;

#[derive(Clone, Copy)]
struct UserFrameEntry {
    pml4_phys: u64,
    count: usize,
    frames: [u64; MAX_FRAMES_PER_PROC],
}

impl UserFrameEntry {
    const fn empty() -> Self {
        Self {
            pml4_phys: 0,
            count: 0,
            frames: [0; MAX_FRAMES_PER_PROC],
        }
    }
}

static TABLE: Mutex<[UserFrameEntry; MAX_PROCS]> =
    Mutex::new([UserFrameEntry::empty(); MAX_PROCS]);

/// Register a frame as owned by the user process whose PML4 root is `pml4_phys`.
/// Returns `false` if no slot is available.
pub fn register(pml4_phys: u64, frame_phys: u64) -> bool {
    if pml4_phys == 0 || frame_phys == 0 {
        return false;
    }
    let mut table = TABLE.lock();
    for entry in table.iter_mut() {
        if entry.pml4_phys == pml4_phys {
            if entry.count < MAX_FRAMES_PER_PROC {
                entry.frames[entry.count] = frame_phys;
                entry.count += 1;
                return true;
            }
            return false;
        }
    }
    for entry in table.iter_mut() {
        if entry.pml4_phys == 0 {
            entry.pml4_phys = pml4_phys;
            entry.frames[0] = frame_phys;
            entry.count = 1;
            return true;
        }
    }
    false
}

/// Release every owned frame for `pml4_phys` and remove the entry.
pub fn release_all(pml4_phys: u64) {
    if pml4_phys == 0 {
        return;
    }
    let mut table = TABLE.lock();
    for entry in table.iter_mut() {
        if entry.pml4_phys == pml4_phys {
            for i in 0..entry.count {
                let phys = entry.frames[i];
                if phys != 0 {
                    deallocate_frame(Frame::from_address(phys as usize));
                }
            }
            *entry = UserFrameEntry::empty();
            return;
        }
    }
}

/// Number of owned leaf frames currently tracked for `pml4_phys`.
/// Useful for diagnostics.
pub fn count_for(pml4_phys: u64) -> usize {
    if pml4_phys == 0 {
        return 0;
    }
    let table = TABLE.lock();
    for entry in table.iter() {
        if entry.pml4_phys == pml4_phys {
            return entry.count;
        }
    }
    0
}
