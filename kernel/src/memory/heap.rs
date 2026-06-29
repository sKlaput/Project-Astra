/// Kernel heap allocator
///
/// Simple bump allocator backed by physical frames
/// Allocates frames and uses them as heap memory
use crate::memory::frame_allocator::allocate_frame;
use crate::memory::paging::{map_page_current, PageTableFlags};
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

const FRAME_SIZE: usize = 4096;
const HEAP_PAGE_WARN_THRESHOLDS: [usize; 4] = [16, 32, 64, 128];

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum HeapPageLimitPolicy {
    WarnOnly,
    PanicOnExceed,
}

// Configure this single constant per phase:
// - `None` disables a hard mapped-page ceiling.
// - `Some((limit_pages, WarnOnly))` logs once when crossing the limit.
// - `Some((limit_pages, PanicOnExceed))` panics immediately on crossing.
const HEAP_PAGE_LIMIT: Option<(usize, HeapPageLimitPolicy)> =
    Some((96, HeapPageLimitPolicy::WarnOnly));
static INJECT_ALLOC_FAILURES: AtomicUsize = AtomicUsize::new(0);
static LAST_ALLOC_FAILURE_WAS_INJECTED: AtomicBool = AtomicBool::new(false);

pub struct HeapTelemetry {
    pub heap_base: usize,
    pub next_virt: usize,
    pub mapped_end: usize,
    pub mapped_pages: usize,
    pub used_bytes: usize,
    pub total_allocated: usize,
}

/// A simple bump allocator backed by physical frames
pub struct BumpAllocator {
    /// First virtual address in heap region.
    heap_base: usize,
    /// Next free virtual address for bump allocations.
    next_virt: usize,
    /// End (exclusive) of virtual range already mapped.
    mapped_end: usize,
    /// Whether heap virtual base has been initialized.
    initialized: bool,
    /// Total bytes allocated
    total_allocated: usize,
    /// Next threshold index for one-shot mapped-page warnings.
    next_warn_threshold_idx: usize,
    /// Whether hard-page-limit warning has already been emitted.
    hard_limit_warned: bool,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        BumpAllocator {
            heap_base: 0,
            next_virt: 0,
            mapped_end: 0,
            initialized: false,
            total_allocated: 0,
            next_warn_threshold_idx: 0,
            hard_limit_warned: false,
        }
    }

    pub fn init(&mut self, heap_base: usize) {
        let base = align_up(heap_base, FRAME_SIZE);
        self.heap_base = base;
        self.next_virt = base;
        self.mapped_end = base;
        self.initialized = true;
        self.next_warn_threshold_idx = 0;
        self.hard_limit_warned = false;
    }

    /// Allocate memory of the given layout
    /// Returns a pointer to the allocated memory or None if allocation failed
    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if !self.initialized {
            return None;
        }

        LAST_ALLOC_FAILURE_WAS_INJECTED.store(false, Ordering::Relaxed);

        if layout.size() != 0
            && INJECT_ALLOC_FAILURES
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                    count.checked_sub(1)
                })
                .is_ok()
        {
            LAST_ALLOC_FAILURE_WAS_INJECTED.store(true, Ordering::Relaxed);
            return None;
        }

        let size = layout.size();
        let align = layout.align();

        // Zero-sized allocations are represented by a non-null dangling pointer.
        if size == 0 {
            return Some(NonNull::dangling());
        }

        let alloc_start = align_up(self.next_virt, align);
        let alloc_end = alloc_start.checked_add(size)?;
        let consumed_bytes = alloc_end.checked_sub(self.next_virt)?;

        while self.mapped_end < alloc_end {
            if !self.can_map_next_page() {
                return None;
            }

            let page_virt = self.mapped_end;
            let frame = allocate_frame()?;
            let flags = PageTableFlags::new(PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
            // Safety: heap pages are mapped to fresh frames from frame allocator,
            // and allocator holds global mutex preventing concurrent mutations.
            unsafe {
                map_page_current(page_virt, frame.start_address(), flags).ok()?;
            }
            self.mapped_end = self.mapped_end.checked_add(FRAME_SIZE)?;
            self.maybe_emit_page_threshold_warning();
        }

        self.next_virt = alloc_end;
        // Track total heap bytes consumed by bump growth (payload + alignment padding).
        self.total_allocated = self.total_allocated.checked_add(consumed_bytes)?;
        NonNull::new(alloc_start as *mut u8)
    }

    fn maybe_emit_page_threshold_warning(&mut self) {
        if self.next_warn_threshold_idx >= HEAP_PAGE_WARN_THRESHOLDS.len() {
            return;
        }

        let mapped_pages = self.mapped_end.saturating_sub(self.heap_base) / FRAME_SIZE;
        let threshold = HEAP_PAGE_WARN_THRESHOLDS[self.next_warn_threshold_idx];
        if mapped_pages >= threshold {
            crate::serial::write_str("heap: warn mapped-pages crossed ");
            crate::serial::write_u64(threshold as u64);
            crate::serial::write_str(" (now ");
            crate::serial::write_u64(mapped_pages as u64);
            crate::serial::write_line(")");
            self.next_warn_threshold_idx += 1;
        }
    }

    fn can_map_next_page(&mut self) -> bool {
        let Some((limit_pages, policy)) = HEAP_PAGE_LIMIT else {
            return true;
        };

        let mapped_pages = self.mapped_end.saturating_sub(self.heap_base) / FRAME_SIZE;
        if mapped_pages < limit_pages {
            return true;
        }

        match policy {
            HeapPageLimitPolicy::WarnOnly => {
                if !self.hard_limit_warned {
                    crate::serial::write_str("heap: warn hard-page-limit reached ");
                    crate::serial::write_u64(limit_pages as u64);
                    crate::serial::write_line(" (warn-only mode)");
                    self.hard_limit_warned = true;
                }
                true
            }
            HeapPageLimitPolicy::PanicOnExceed => {
                crate::serial::write_str("heap: FATAL hard-page-limit reached ");
                crate::serial::write_u64(limit_pages as u64);
                crate::serial::write_line(" (panic mode)");
                panic!("heap hard-page-limit exceeded")
            }
        }
    }

    /// Deallocate memory (no-op for bump allocator)
    pub fn deallocate(&mut self, _ptr: NonNull<u8>, _layout: Layout) {
        // Bump allocator doesn't free individual allocations
        // This is a limitation we accept for now
    }

    /// Get the total bytes allocated so far
    pub fn total_allocated(&self) -> usize {
        self.total_allocated
    }

    pub fn telemetry(&self) -> HeapTelemetry {
        let mapped_pages = if self.initialized {
            self.mapped_end.saturating_sub(self.heap_base) / FRAME_SIZE
        } else {
            0
        };
        let used_bytes = if self.initialized {
            self.next_virt.saturating_sub(self.heap_base)
        } else {
            0
        };

        HeapTelemetry {
            heap_base: self.heap_base,
            next_virt: self.next_virt,
            mapped_end: self.mapped_end,
            mapped_pages,
            used_bytes,
            total_allocated: self.total_allocated,
        }
    }
}

/// Global kernel heap allocator instance
static HEAP_ALLOCATOR: Mutex<BumpAllocator> = Mutex::new(BumpAllocator::new());

/// Global allocator for the kernel
pub struct KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        HEAP_ALLOCATOR
            .lock()
            .allocate(layout)
            .map(|nn| nn.as_ptr())
            .unwrap_or(core::ptr::null_mut())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !ptr.is_null() {
            unsafe {
                HEAP_ALLOCATOR
                    .lock()
                    .deallocate(NonNull::new_unchecked(ptr), layout);
            }
        }
    }
}

/// Report heap allocation status
pub fn report_heap_status() {
    let total = HEAP_ALLOCATOR.lock().total_allocated();
    crate::serial::write_str("heap: allocated ");
    crate::serial::write_u64(total as u64);
    crate::serial::write_line(" bytes");
}

pub fn get_telemetry() -> HeapTelemetry {
    HEAP_ALLOCATOR.lock().telemetry()
}

pub fn report_heap_telemetry() {
    let snapshot = HEAP_ALLOCATOR.lock().telemetry();

    crate::serial::write_str("heap: telemetry pages=");
    crate::serial::write_u64(snapshot.mapped_pages as u64);
    crate::serial::write_str(" used=");
    crate::serial::write_u64(snapshot.used_bytes as u64);
    crate::serial::write_str(" total=");
    crate::serial::write_u64(snapshot.total_allocated as u64);
    crate::serial::write_line("");

    crate::serial::write_str("heap: telemetry base=0x");
    crate::serial::write_u64(snapshot.heap_base as u64);
    crate::serial::write_str(" next=0x");
    crate::serial::write_u64(snapshot.next_virt as u64);
    crate::serial::write_str(" mapped-end=0x");
    crate::serial::write_u64(snapshot.mapped_end as u64);
    crate::serial::write_line("");
}

pub fn init_virtual_heap(heap_base: usize) {
    let mut heap = HEAP_ALLOCATOR.lock();
    heap.init(heap_base);
    crate::serial::write_str("heap: virtual base=0x");
    crate::serial::write_u64(heap_base as u64);
    crate::serial::write_line("");

    match HEAP_PAGE_LIMIT {
        Some((limit, HeapPageLimitPolicy::WarnOnly)) => {
            crate::serial::write_str("heap: hard-page-limit=");
            crate::serial::write_u64(limit as u64);
            crate::serial::write_line(" (warn-only)");
        }
        Some((limit, HeapPageLimitPolicy::PanicOnExceed)) => {
            crate::serial::write_str("heap: hard-page-limit=");
            crate::serial::write_u64(limit as u64);
            crate::serial::write_line(" (panic-on-exceed)");
        }
        None => crate::serial::write_line("heap: hard-page-limit disabled"),
    }
}

pub fn inject_alloc_failures(count: usize) {
    LAST_ALLOC_FAILURE_WAS_INJECTED.store(false, Ordering::Relaxed);
    INJECT_ALLOC_FAILURES.store(count, Ordering::Relaxed);
}

pub fn last_alloc_failure_was_injected() -> bool {
    LAST_ALLOC_FAILURE_WAS_INJECTED.load(Ordering::Relaxed)
}

fn align_up(addr: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (addr + align - 1) & !(align - 1)
}
