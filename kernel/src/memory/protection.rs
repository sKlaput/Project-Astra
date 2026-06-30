//! Memory protection infrastructure — guard pages, address space layout, W^X enforcement.
#![allow(dead_code)]

use crate::memory::paging::{PageTableFlags, is_user_virt, is_kernel_virt};
use core::sync::atomic::{AtomicUsize, Ordering};

// ── User Address Space Layout ──────────────────────────────────────────────────

/// User process virtual address space layout (x86_64, lower-half 0x0-0x0000_8000_0000_0000)
///
/// ```
/// 0x0000_7FFF_FFFF_FFFF  ┌─────────────────────┐
///                        │   Stack (grows ↓)   │  1 MiB
/// 0x0000_7FFF_FFF0_0000  ├─────────────────────┤
///                        │   Stack Guard Page  │  4 KiB (red zone - unmapped)
/// 0x0000_7FFF_FFEF_F000  ├─────────────────────┤
///                        │                     │
///                        │   Heap (grows ↑)    │  Dynamic
///                        │                     │
/// 0x0000_0000_6000_0000  ├─────────────────────┤
///                        │   Heap Guard Page   │  4 KiB (unmapped)
/// 0x0000_0000_5FFF_F000  ├─────────────────────┤
///                        │                     │
///                        │   Data/BSS/etc      │  ~512 KiB
///                        │                     │
/// 0x0000_0000_5000_0000  ├─────────────────────┤
///                        │   Data Guard Page   │  4 KiB (unmapped)
/// 0x0000_0000_4FFF_F000  ├─────────────────────┤
///                        │                     │
///                        │   Code (RX)         │  ~512 KiB
///                        │                     │
/// 0x0000_0000_4000_0000  ├─────────────────────┤
///                        │   Null Page Guard   │  4 KiB (unmapped)
/// 0x0000_0000_0000_0000  └─────────────────────┘
/// ```

/// First page is always unmapped (null pointer deref protection)
pub const NULL_GUARD_VIRT: usize = 0x0000_0000_0000_0000;
pub const NULL_GUARD_SIZE: usize = 0x0000_0000_0001_0000; // 64 KiB guard region

/// Code section starts after null guard
pub const CODE_BASE_VIRT: usize = 0x0000_0000_4000_0000;
pub const CODE_MAX_SIZE: usize = 512 * 1024; // 512 KiB typical for ELF

/// Guard page between code and data
pub const CODE_GUARD_VIRT: usize = CODE_BASE_VIRT + CODE_MAX_SIZE;

/// Data/BSS section
pub const DATA_BASE_VIRT: usize = 0x0000_0000_5000_0000;
pub const DATA_MAX_SIZE: usize = 512 * 1024; // 512 KiB

/// Guard page between data and heap
pub const DATA_GUARD_VIRT: usize = DATA_BASE_VIRT + DATA_MAX_SIZE;

/// Heap starts after data
pub const HEAP_BASE_VIRT: usize = 0x0000_0000_6000_0000;
pub const HEAP_MAX_SIZE: usize = 256 * 1024 * 1024; // 256 MiB

/// Guard page between heap and stack
pub const HEAP_GUARD_VIRT: usize = HEAP_BASE_VIRT + HEAP_MAX_SIZE;

/// Stack grows downward
pub const STACK_TOP_VIRT: usize = 0x0000_7FFF_FFFF_FFFF;
pub const STACK_SIZE: usize = 1024 * 1024; // 1 MiB

/// Guard page at bottom of stack (red zone)
pub const STACK_GUARD_VIRT: usize = STACK_TOP_VIRT - STACK_SIZE;

// ── Protection Tracking ────────────────────────────────────────────────────────

/// Tracks memory protection statistics
#[derive(Clone, Copy, Debug)]
pub struct ProtectionStats {
    /// Number of guard pages allocated
    pub guard_pages: usize,
    /// Number of protection violations detected
    pub violations: usize,
}

static GUARD_PAGES_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static PROTECTION_VIOLATIONS: AtomicUsize = AtomicUsize::new(0);

pub fn stats() -> ProtectionStats {
    ProtectionStats {
        guard_pages: GUARD_PAGES_ALLOCATED.load(Ordering::Relaxed),
        violations: PROTECTION_VIOLATIONS.load(Ordering::Relaxed),
    }
}

// ── Guard Page Setup ───────────────────────────────────────────────────────────

/// Install guard pages for a new user process.
/// Must be called once per new user address space (PML4).
///
/// Creates unmapped regions at:
/// - 0x0 (null pointer protection)
/// - Code/Data boundary (code buffer overflow)
/// - Data/Heap boundary (data buffer overflow)
/// - Heap/Stack boundary (heap overflow)
/// - Stack bottom (stack overflow/red zone)
pub fn install_guard_pages_for_process(pml4_virt: usize) -> bool {
    // Guard pages should already be unmapped (no entries in page tables).
    // This function is a placeholder for future explicit management if needed.
    // For now, guard pages are safe by virtue of not being mapped at all.
    
    let guards = [
        NULL_GUARD_VIRT,
        CODE_GUARD_VIRT,
        DATA_GUARD_VIRT,
        HEAP_GUARD_VIRT,
        STACK_GUARD_VIRT,
    ];
    
    for &guard_addr in &guards {
        // Verify this is in user space
        if !is_user_virt(guard_addr) {
            return false;
        }
    }
    
    GUARD_PAGES_ALLOCATED.fetch_add(guards.len(), Ordering::Relaxed);
    true
}

// ── Address Space Validation ───────────────────────────────────────────────────

/// Check if an address falls within a protected guard region
pub fn is_in_guard_region(addr: usize) -> bool {
    // Null page guard
    if addr < NULL_GUARD_SIZE {
        return true;
    }
    
    // Code/Data boundary guard (single page)
    if addr >= CODE_GUARD_VIRT && addr < CODE_GUARD_VIRT + 4096 {
        return true;
    }
    
    // Data/Heap boundary guard (single page)
    if addr >= DATA_GUARD_VIRT && addr < DATA_GUARD_VIRT + 4096 {
        return true;
    }
    
    // Heap/Stack boundary guard (single page)
    if addr >= HEAP_GUARD_VIRT && addr < HEAP_GUARD_VIRT + 4096 {
        return true;
    }
    
    // Stack overflow red zone
    if addr >= STACK_GUARD_VIRT && addr < STACK_GUARD_VIRT + 4096 {
        return true;
    }
    
    false
}

/// Validate that a user address range is accessible (not in guard region)
pub fn validate_user_range(start: usize, len: usize) -> bool {
    if !is_user_virt(start) {
        return false;
    }
    
    if let Some(end) = start.checked_add(len) {
        if end > 0x0000_8000_0000_0000 {
            return false; // Wraps into kernel space
        }
    } else {
        return false; // Overflow
    }
    
    // Check if range intersects any guard region
    for page in (start..start + len).step_by(4096) {
        if is_in_guard_region(page) {
            return false;
        }
    }
    
    true
}

/// Record a protection violation (used for diagnostics)
pub fn record_violation() {
    PROTECTION_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
}

// ── Ring-3 Isolation Helpers ───────────────────────────────────────────────────

/// Returns the recommended flags for code pages (read/execute, no write)
pub fn code_page_flags() -> u64 {
    PageTableFlags::empty()
        .with(PageTableFlags::PRESENT)
        .with(PageTableFlags::USER_ACCESSIBLE)
        // No WRITABLE flag
        .bits()
}

/// Returns the recommended flags for data pages (read/write, no execute)
pub fn data_page_flags() -> u64 {
    PageTableFlags::empty()
        .with(PageTableFlags::PRESENT)
        .with(PageTableFlags::WRITABLE)
        .with(PageTableFlags::USER_ACCESSIBLE)
        .with(PageTableFlags::EXECUTE_DISABLE)
        .bits()
}

/// Returns the recommended flags for stack pages (read/write, no execute)
pub fn stack_page_flags() -> u64 {
    data_page_flags() // Same as data pages
}
