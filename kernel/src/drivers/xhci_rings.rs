// ---------------------------------------------------------------------------
// XHCI Ring Management
//
// Implements ring buffer allocation and management for:
//   - Command Ring (host→controller commands)
//   - Event Ring (controller→host completions)
//   - Endpoint Rings (data transfers)
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicU32, Ordering};

/// XHCI TRB (Transfer Request Block) - 16 bytes
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Trb {
    pub data_low: u32,
    pub data_high: u32,
    pub status: u32,
    pub control: u32,
}

impl Trb {
    pub fn new() -> Self {
        Self {
            data_low: 0,
            data_high: 0,
            status: 0,
            control: 0,
        }
    }

    /// Set cycle bit (toggles on wrap-around)
    pub fn set_cycle(&mut self, cycle: bool) {
        if cycle {
            self.control |= 0x00000001;
        } else {
            self.control &= !0x00000001;
        }
    }

    /// Get cycle bit
    pub fn get_cycle(&self) -> bool {
        (self.control & 0x00000001) != 0
    }

    /// Set TRB type
    pub fn set_type(&mut self, trb_type: u8) {
        self.control = (self.control & 0x0000003F) | ((trb_type as u32) << 10);
    }
}

/// Ring buffer for command/event/endpoint transfers
pub struct Ring {
    /// Virtual address of ring (must be page-aligned)
    buffer: u64,
    /// Ring size in TRBs (typically 256)
    size: u32,
    /// Current enqueue pointer (for command/endpoint rings)
    enqueue: AtomicU32,
    /// Current dequeue pointer (for event rings)
    dequeue: AtomicU32,
    /// Cycle state bit
    cycle: AtomicU32,
}

impl Ring {
    /// Create a new ring (buffer must be pre-allocated and page-aligned)
    pub fn new(buffer: u64, size: u32) -> Self {
        Self {
            buffer,
            size,
            enqueue: AtomicU32::new(0),
            dequeue: AtomicU32::new(0),
            cycle: AtomicU32::new(1),
        }
    }

    /// Get the address of a TRB at the given index
    fn get_trb_addr(&self, index: u32) -> u64 {
        self.buffer + ((index % self.size) as u64 * 16)
    }

    /// Get TRB at index
    pub fn get_trb(&self, index: u32) -> &'static mut Trb {
        let addr = self.get_trb_addr(index) as *mut Trb;
        unsafe { &mut *addr }
    }

    /// Enqueue a TRB (for command/endpoint rings)
    pub fn enqueue_trb(&self, trb: &Trb) -> u32 {
        let enq = self.enqueue.load(Ordering::Relaxed);
        let cycle = self.cycle.load(Ordering::Relaxed) != 0;

        let dest = self.get_trb(enq);
        *dest = *trb;
        dest.set_cycle(cycle);

        let next_enq = (enq + 1) % self.size;
        if next_enq == 0 {
            // Wrap around - toggle cycle
            self.cycle.store(if cycle { 0 } else { 1 }, Ordering::Release);
        }
        self.enqueue.store(next_enq, Ordering::Release);

        enq
    }

    /// Dequeue a TRB (for event rings)
    pub fn dequeue_trb(&self) -> Option<&'static mut Trb> {
        let deq = self.dequeue.load(Ordering::Relaxed);
        let cycle = self.cycle.load(Ordering::Relaxed) != 0;

        let trb = self.get_trb(deq);
        if trb.get_cycle() != cycle {
            return None;  // No new events
        }

        let next_deq = (deq + 1) % self.size;
        if next_deq == 0 {
            // Wrap around - toggle cycle
            self.cycle.store(if cycle { 0 } else { 1 }, Ordering::Release);
        }
        self.dequeue.store(next_deq, Ordering::Release);

        Some(trb)
    }

    /// Get ring base address for hardware register
    pub fn get_base_address(&self) -> u64 {
        self.buffer
    }

    /// Get enqueue pointer
    pub fn enqueue_ptr(&self) -> u32 {
        self.enqueue.load(Ordering::Relaxed)
    }

    /// Get dequeue pointer
    pub fn dequeue_ptr(&self) -> u32 {
        self.dequeue.load(Ordering::Relaxed)
    }

    /// Get cycle state
    pub fn cycle(&self) -> bool {
        self.cycle.load(Ordering::Relaxed) != 0
    }
}

/// Allocate a ring buffer
/// Note: In production, this would allocate from a page-aligned allocator
pub fn allocate_ring(size: u32) -> Option<Ring> {
    // For now, return None (allocation would happen from kernel allocator)
    // In full implementation, would:
    // 1. Allocate (size * 16) bytes aligned to page boundary
    // 2. Initialize all TRBs to 0
    // 3. Return Ring structure
    
    crate::serial::write_str("xhci: Ring allocation requested for ");
    crate::serial::write_u32(size);
    crate::serial::write_line(" TRBs (not yet implemented)");
    
    None
}
