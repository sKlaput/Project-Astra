// ---------------------------------------------------------------------------
// PS/2 Keyboard Driver  (IRQ1 / scancode set 1)
//
// Implements the `Driver` trait.  On `init()`:
//   1. Registers `ps2_keyboard_irq_handler` with the interrupt layer.
//   2. Unmasks IRQ1 on the PIC so the keyboard can fire interrupts.
//
// Scancodes are stored in a 64-entry lock-free ring buffer.
// Readers call `read_scancode()` to pop the oldest byte, or `None` if empty.
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use super::{Driver, DriverError};

// ---------------------------------------------------------------------------
// Ring buffer (64 entries, single-producer from ISR, single-consumer)
// ---------------------------------------------------------------------------

const KB_BUF_SIZE: usize = 64;

static KB_BUF: [AtomicU8; KB_BUF_SIZE] = {
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; KB_BUF_SIZE]
};

static KB_HEAD: AtomicUsize = AtomicUsize::new(0); // next write position
static KB_TAIL: AtomicUsize = AtomicUsize::new(0); // next read  position

/// Push a raw scancode byte into the ring buffer.  Called from the ISR.
/// If the buffer is full the oldest byte is silently dropped.
fn push_scancode(byte: u8) {
    let head = KB_HEAD.load(Ordering::Relaxed);
    let next_head = (head + 1) % KB_BUF_SIZE;
    let tail = KB_TAIL.load(Ordering::Acquire);
    if next_head == tail {
        // Buffer full: advance tail to discard the oldest entry.
        KB_TAIL.store((tail + 1) % KB_BUF_SIZE, Ordering::Release);
    }
    KB_BUF[head].store(byte, Ordering::Relaxed);
    KB_HEAD.store(next_head, Ordering::Release);
}

/// Public entry point for non-ISR callers (e.g. mouse poll draining keyboard
/// bytes from the shared PS/2 FIFO).  Same logic as the private push.
pub fn push_scancode_from_poll(byte: u8) {
    push_scancode(byte);
}

/// Pop the oldest scancode byte, or `None` if the buffer is empty.
pub fn read_scancode() -> Option<u8> {
    let tail = KB_TAIL.load(Ordering::Acquire);
    let head = KB_HEAD.load(Ordering::Acquire);
    if tail == head {
        return None;
    }
    let byte = KB_BUF[tail].load(Ordering::Relaxed);
    KB_TAIL.store((tail + 1) % KB_BUF_SIZE, Ordering::Release);
    Some(byte)
}

/// Number of scancodes currently waiting in the buffer.
pub fn scancode_count() -> usize {
    let head = KB_HEAD.load(Ordering::Acquire);
    let tail = KB_TAIL.load(Ordering::Acquire);
    if head >= tail {
        head - tail
    } else {
        KB_BUF_SIZE - tail + head
    }
}

// ---------------------------------------------------------------------------
// IRQ1 dispatch target — registered with the interrupt layer at init time.
// ---------------------------------------------------------------------------

fn ps2_keyboard_irq_handler(scancode: u8) {
    push_scancode(scancode);
}

// ---------------------------------------------------------------------------
// Driver trait implementation
// ---------------------------------------------------------------------------

pub struct Ps2KeyboardDriver;

impl Driver for Ps2KeyboardDriver {
    fn name(&self) -> &'static str {
        "ps2-keyboard"
    }

    fn category(&self) -> &'static str {
        "input"
    }

    fn init(&self) -> Result<(), DriverError> {
        crate::arch::x86_64::interrupts::register_keyboard_handler(ps2_keyboard_irq_handler);
        crate::arch::x86_64::interrupts::unmask_keyboard_irq();
        crate::serial::write_line("drivers: ps2-keyboard initialised (IRQ1 unmasked)");
        Ok(())
    }
}
