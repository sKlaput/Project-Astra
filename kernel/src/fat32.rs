// ---------------------------------------------------------------------------
// Astra OS — FAT32 read-only driver (Step 2)
//
// Mounts the first FAT32 partition found on the virtio-blk device.
// Provides:
//   fat32::mount()          — parse BPB, verify FAT32, store geometry
//   fat32::list_dir()       — enumerate entries in a directory cluster
//   fat32::read_file()      — read file contents by start cluster + size
//   fat32::root_cluster()   — returns the root directory cluster number
//
// Design constraints:
//   - No heap allocations — all buffers are stack-local or static
//   - Read-only; writes not implemented
//   - Supports long-file-name (LFN) entries — they are silently skipped
//     so short 8.3 names always show correctly
//   - Cluster cache: one 512-byte static sector buffer (no allocation needed
//     for small sequential reads; callers must copy data out before next call)
// ---------------------------------------------------------------------------

use crate::drivers::virtio_blk;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

// ── FAT32 mount state ─────────────────────────────────────────────────────────

static MOUNTED: AtomicBool = AtomicBool::new(false);
static BYTES_PER_SEC: AtomicU32 = AtomicU32::new(512);
static SEC_PER_CLUS: AtomicU32 = AtomicU32::new(0);
static RSVD_SECS: AtomicU32 = AtomicU32::new(0);
static NUM_FATS: AtomicU32 = AtomicU32::new(0);
static FAT_SIZE: AtomicU32 = AtomicU32::new(0); // sectors per FAT
static DATA_START: AtomicU32 = AtomicU32::new(0); // first data sector
static ROOT_CLUS: AtomicU32 = AtomicU32::new(0);
static PART_START: AtomicU64 = AtomicU64::new(0); // LBA of partition sector 0

include!("fat32/mount.rs");
include!("fat32/directory.rs");
include!("fat32/write.rs");
include!("fat32/mkfs.rs");
