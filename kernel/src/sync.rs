// ---------------------------------------------------------------------------
// Kernel synchronization primitives built on cooperative scheduling.
// ---------------------------------------------------------------------------
use crate::scheduler::{self, TaskId};
use core::sync::atomic::{AtomicU64, Ordering};

// Maximum number of tasks that can queue on a single mutex at once.
const WAIT_CAP: usize = 8;

include!("sync/locks.rs");
include!("sync/semaphore.rs");
include!("sync/channel.rs");
include!("sync/condvar.rs");
include!("sync/rwlock.rs");
