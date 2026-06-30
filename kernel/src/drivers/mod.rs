// ---------------------------------------------------------------------------
// Kernel Driver Model
//
// All drivers implement the `Driver` trait.  A fixed-capacity static registry
// (`DRIVER_REGISTRY`) holds references to every registered driver so the
// kernel can initialise, query, and enumerate them without heap iteration.
// ---------------------------------------------------------------------------

pub mod block;
pub mod keyboard;
pub mod mouse;
pub mod virtio_blk;
pub mod virtio_net;
pub mod virtio_common;

use core::sync::atomic::{AtomicUsize, Ordering};

/// Unified error type surfaced by all driver operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    /// Device not present or hardware did not respond.
    DeviceNotPresent,
    /// Caller supplied an address/index outside the valid range.
    OutOfRange,
    /// Operation not supported by this driver.
    Unsupported,
    /// Internal hardware or protocol error.
    IoError,
    /// Driver has not been initialised yet.
    NotInitialised,
    /// Registry is full; no more drivers can be registered.
    RegistryFull,
}

/// Core driver interface.  Every driver must implement this.
pub trait Driver: Sync {
    /// Short ASCII name used in init logs (no heap allocation required).
    fn name(&self) -> &'static str;

    /// Initialise the hardware.  Called once during kernel boot.
    /// Returns `Ok(())` on success or a `DriverError` on failure.
    fn init(&self) -> Result<(), DriverError>;

    /// Category string for enumeration: `"char"`, `"block"`, `"input"`, etc.
    fn category(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// Simple static driver registry
// ---------------------------------------------------------------------------

const REGISTRY_CAP: usize = 16;

static REGISTRY_LEN: AtomicUsize = AtomicUsize::new(0);
// We store fat-pointer-sized pairs (data ptr + vtable ptr) as two u64s each.
// On x86_64 a `&dyn Driver` is 16 bytes (2 × usize).
// We encode and decode manually to avoid `static mut` and unstable features.
struct DriverSlot {
    data: core::sync::atomic::AtomicU64,
    vtbl: core::sync::atomic::AtomicU64,
}

impl DriverSlot {
    const fn empty() -> Self {
        Self {
            data: core::sync::atomic::AtomicU64::new(0),
            vtbl: core::sync::atomic::AtomicU64::new(0),
        }
    }
}

// SAFETY: DriverSlot only holds AtomicU64 values; no interior mutability
// beyond what AtomicU64 provides, which is Send + Sync.
unsafe impl Sync for DriverSlot {}

static REGISTRY: [DriverSlot; REGISTRY_CAP] = {
    // Workaround: init array of non-Copy types with a const fn helper.
    // `const {}` block inside array init is stable on the nightly toolchain.
    const EMPTY: DriverSlot = DriverSlot::empty();
    [EMPTY; REGISTRY_CAP]
};

/// Register a driver in the global registry and call its `init()`.
///
/// Returns `Ok(index)` on success.  The caller keeps ownership of the
/// `&'static dyn Driver` reference; the registry only stores the fat pointer.
pub fn register(driver: &'static dyn Driver) -> Result<usize, DriverError> {
    let idx = REGISTRY_LEN.fetch_add(1, Ordering::Relaxed);
    if idx >= REGISTRY_CAP {
        REGISTRY_LEN.store(REGISTRY_CAP, Ordering::Relaxed);
        return Err(DriverError::RegistryFull);
    }

    // Decompose the fat pointer into its two word-sized halves.
    // SAFETY: `&dyn Driver` is a fat pointer consisting of (data_ptr, vtable_ptr).
    let (data_ptr, vtbl_ptr): (usize, usize) = unsafe {
        let raw: [usize; 2] = core::mem::transmute(driver);
        (raw[0], raw[1])
    };

    REGISTRY[idx].data.store(data_ptr as u64, Ordering::Release);
    REGISTRY[idx].vtbl.store(vtbl_ptr as u64, Ordering::Release);

    driver.init().map(|()| idx)
}

/// Number of drivers currently registered.
pub fn registered_count() -> usize {
    REGISTRY_LEN.load(Ordering::Relaxed).min(REGISTRY_CAP)
}

/// Iterate over every registered driver, calling `f(index, &dyn Driver)`.
pub fn for_each(mut f: impl FnMut(usize, &'static dyn Driver)) {
    let n = registered_count();
    for i in 0..n {
        let data = REGISTRY[i].data.load(Ordering::Acquire) as usize;
        let vtbl = REGISTRY[i].vtbl.load(Ordering::Acquire) as usize;
        if data == 0 || vtbl == 0 {
            continue;
        }
        // SAFETY: stored in `register()` from a valid `&'static dyn Driver`.
        let driver: &'static dyn Driver = unsafe { core::mem::transmute([data, vtbl]) };
        f(i, driver);
    }
}
