use crate::serial;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

/// Frame size in bytes (4 KiB, standard x86_64 page size)
pub const FRAME_SIZE: usize = 4096;

/// Number of bits in the bitmap (supports tracking up to 512 MB of memory)
const BITMAP_SIZE: usize = 65536; // 64 KB bitmap = 512 MB of frames

/// A single frame (page) address
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Frame {
    pub number: usize,
}

impl Frame {
    /// Create a Frame from a frame number
    pub fn new(number: usize) -> Self {
        Frame { number }
    }

    /// Get the physical address of this frame
    pub fn start_address(&self) -> usize {
        self.number * FRAME_SIZE
    }

    /// Get the frame number from a physical address
    pub fn from_address(addr: usize) -> Self {
        Frame {
            number: addr / FRAME_SIZE,
        }
    }
}

/// Bitmap-based frame allocator
pub struct BitmapFrameAllocator {
    bitmap: [u8; BITMAP_SIZE],
    next_search: AtomicUsize,
}

impl BitmapFrameAllocator {
    /// Create a new, empty frame allocator
    pub const fn new() -> Self {
        BitmapFrameAllocator {
            bitmap: [0; BITMAP_SIZE],
            next_search: AtomicUsize::new(0),
        }
    }

    /// Mark a frame as available for allocation
    pub fn mark_frame_available(&mut self, frame: Frame) {
        let byte_idx = frame.number / 8;
        let bit_idx = frame.number % 8;

        if byte_idx < BITMAP_SIZE {
            // Set bit to 1 = available
            self.bitmap[byte_idx] |= 1 << bit_idx;
        }
    }

    /// Mark a range of frames as available
    pub fn mark_range_available(&mut self, start_addr: usize, len: usize) {
        let start_frame = start_addr / FRAME_SIZE;
        let num_frames = (len + FRAME_SIZE - 1) / FRAME_SIZE;

        for i in 0..num_frames {
            let frame_num = start_frame + i;
            let byte_idx = frame_num / 8;
            let bit_idx = frame_num % 8;

            if byte_idx < BITMAP_SIZE {
                self.bitmap[byte_idx] |= 1 << bit_idx;
            }
        }
    }

    /// Allocate a single frame, returning None if no frames available
    pub fn allocate_frame(&mut self) -> Option<Frame> {
        let start = self.next_search.load(Ordering::SeqCst);

        for byte_idx in start..BITMAP_SIZE {
            if self.bitmap[byte_idx] != 0 {
                // Found a byte with at least one available bit
                for bit_idx in 0..8 {
                    if (self.bitmap[byte_idx] & (1 << bit_idx)) != 0 {
                        // Clear the bit (mark as allocated)
                        self.bitmap[byte_idx] &= !(1 << bit_idx);

                        let frame_num = byte_idx * 8 + bit_idx;
                        self.next_search.store(byte_idx, Ordering::SeqCst);

                        return Some(Frame::new(frame_num));
                    }
                }
            }
        }

        // Wrap around to the beginning if we reach the end
        for byte_idx in 0..start {
            if self.bitmap[byte_idx] != 0 {
                for bit_idx in 0..8 {
                    if (self.bitmap[byte_idx] & (1 << bit_idx)) != 0 {
                        self.bitmap[byte_idx] &= !(1 << bit_idx);
                        let frame_num = byte_idx * 8 + bit_idx;
                        self.next_search.store(byte_idx, Ordering::SeqCst);
                        return Some(Frame::new(frame_num));
                    }
                }
            }
        }

        None
    }

    /// Deallocate a frame, marking it as available again
    pub fn deallocate_frame(&mut self, frame: Frame) {
        let byte_idx = frame.number / 8;
        let bit_idx = frame.number % 8;

        if byte_idx < BITMAP_SIZE {
            // Set bit to 1 = available
            self.bitmap[byte_idx] |= 1 << bit_idx;
        }
    }

    /// Get the total number of available frames
    pub fn available_frames(&self) -> usize {
        let mut count = 0;
        for byte in &self.bitmap {
            for bit in 0..8 {
                if (byte & (1 << bit)) != 0 {
                    count += 1;
                }
            }
        }
        count
    }

    /// Get the total physical memory we can track (in bytes)
    pub fn max_trackable_memory(&self) -> usize {
        BITMAP_SIZE * 8 * FRAME_SIZE
    }
}

/// Global frame allocator instance
static FRAME_ALLOCATOR: Mutex<BitmapFrameAllocator> = Mutex::new(BitmapFrameAllocator::new());

/// Initialize the frame allocator with available memory regions from boot
pub fn init_from_memory_map(regions: &[(usize, usize)]) {
    const MIN_USABLE_PHYS_ADDR: usize = 0x10_0000; // 1 MiB

    let mut allocator = FRAME_ALLOCATOR.lock();

    for (start_addr, len) in regions {
        let region_start = (*start_addr).max(MIN_USABLE_PHYS_ADDR);
        let region_end = start_addr.saturating_add(*len);

        if region_end <= region_start {
            continue;
        }

        let filtered_len = region_end - region_start;

        serial::write_str("  frame_allocator: marking range 0x");
        serial::write_u64(region_start as u64);
        serial::write_str(" len 0x");
        serial::write_u64(filtered_len as u64);
        serial::write_line("");
        allocator.mark_range_available(region_start, filtered_len);
    }

    let available = allocator.available_frames();
    let available_bytes = available * FRAME_SIZE;
    let available_mb = available_bytes / (1024 * 1024);
    serial::write_str("frame_allocator: initialized with ");
    serial::write_u64(available as u64);
    serial::write_str(" frames (");
    serial::write_u64(available_mb as u64);
    serial::write_line(" MB)");
}

/// Allocate a single frame from the global allocator
pub fn allocate_frame() -> Option<Frame> {
    FRAME_ALLOCATOR.lock().allocate_frame()
}

/// Deallocate a frame back to the global allocator
pub fn deallocate_frame(frame: Frame) {
    FRAME_ALLOCATOR.lock().deallocate_frame(frame);
}

/// Get the number of available frames
pub fn available_frames() -> usize {
    FRAME_ALLOCATOR.lock().available_frames()
}
