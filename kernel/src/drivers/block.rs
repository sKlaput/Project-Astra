// ---------------------------------------------------------------------------
// RAM Block Driver
//
// A stub block device backed by a 512-byte static array.  It satisfies the
// block-device category requirement for E6 and provides a verified read/write
// path without needing real storage hardware.
//
// Block addressing: only block 0 is supported (single-sector device).
// ---------------------------------------------------------------------------

use super::{Driver, DriverError};
use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;

// ---------------------------------------------------------------------------
// 512-byte backing store
// ---------------------------------------------------------------------------

const BLOCK_SIZE: usize = 512;

struct BlockBuf(UnsafeCell<[u8; BLOCK_SIZE]>);

// SAFETY: access is guarded by `BLOCK_LOCKED` spin-flag; no concurrent writes
// can occur at the same time as a read.
unsafe impl Sync for BlockBuf {}

static BLOCK_DATA: BlockBuf = BlockBuf(UnsafeCell::new([0u8; BLOCK_SIZE]));
static BLOCK_LOCKED: AtomicBool = AtomicBool::new(false);

/// Acquire the single-block lock (spin).
fn lock_block() {
    while BLOCK_LOCKED.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

fn unlock_block() {
    BLOCK_LOCKED.store(false, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Public block I/O API
// ---------------------------------------------------------------------------

pub struct RamBlockDriver;

impl RamBlockDriver {
    /// Read block `index` into `buf`.  Only block 0 exists.
    pub fn read_block(&self, index: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), DriverError> {
        if index != 0 {
            return Err(DriverError::OutOfRange);
        }
        lock_block();
        // SAFETY: lock guarantees exclusive access.
        let src = unsafe { &*BLOCK_DATA.0.get() };
        buf.copy_from_slice(src);
        unlock_block();
        Ok(())
    }

    /// Write `data` into block `index`.  Only block 0 exists.
    pub fn write_block(&self, index: u64, data: &[u8; BLOCK_SIZE]) -> Result<(), DriverError> {
        if index != 0 {
            return Err(DriverError::OutOfRange);
        }
        lock_block();
        // SAFETY: lock guarantees exclusive access.
        let dst = unsafe { &mut *BLOCK_DATA.0.get() };
        dst.copy_from_slice(data);
        unlock_block();
        Ok(())
    }
}

impl Driver for RamBlockDriver {
    fn name(&self) -> &'static str {
        "ram-block"
    }

    fn category(&self) -> &'static str {
        "block"
    }

    fn init(&self) -> Result<(), DriverError> {
        // Pre-stamp the block with a recognisable header so a subsequent
        // read can verify the write path worked.
        let header = b"RAMBLK00";
        lock_block();
        // SAFETY: lock guarantees exclusive access.
        let dst = unsafe { &mut *BLOCK_DATA.0.get() };
        dst[..8].copy_from_slice(header);
        unlock_block();
        crate::serial::write_line("drivers: ram-block initialised (1 x 512-byte block)");
        Ok(())
    }
}
