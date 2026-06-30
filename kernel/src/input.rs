// ---------------------------------------------------------------------------
// Input event system
//
// Translates raw PS/2 scancode-set-1 bytes and PS/2 mouse packets into a
// unified `Event` stream that the desktop compositor can consume.
//
// API
//   `poll_events(buf)` — fills `buf` with pending events. Non-blocking.
// ---------------------------------------------------------------------------

use crate::drivers::keyboard;
use crate::drivers::mouse;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};

// ── Key codes ─────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Key {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Char(u8), // printable (shift-aware)
    Ctrl(u8), // Ctrl+key
    Unknown(u8),
}

static SHIFT_HELD: AtomicBool = AtomicBool::new(false);
static CTRL_HELD: AtomicBool = AtomicBool::new(false);

// ── Mouse state ───────────────────────────────────────────────────────────────

static MOUSE_X: AtomicI32 = AtomicI32::new(512);
static MOUSE_Y: AtomicI32 = AtomicI32::new(384);
static MOUSE_BUTTONS: AtomicU8 = AtomicU8::new(0);

// ── Event type ────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Event {
    KeyPress(Key),
    MouseMove(i32, i32),
    MouseButton(u8),
    /// Scroll wheel delta: positive = scroll up, negative = scroll down.
    MouseScroll(i32),
}

// ── Scancode decoding ─────────────────────────────────────────────────────────

static EXTENDED: AtomicBool = AtomicBool::new(false);

fn shifted(lo: u8, hi: u8) -> u8 {
    if SHIFT_HELD.load(Ordering::Relaxed) {
        hi
    } else {
        lo
    }
}

fn decode_scancode(byte: u8) -> Option<Key> {
    // Handle Shift press/release
    match byte {
        0x2A | 0x36 => {
            SHIFT_HELD.store(true, Ordering::Relaxed);
            return None;
        }
        0xAA | 0xB6 => {
            SHIFT_HELD.store(false, Ordering::Relaxed);
            return None;
        }
        0x1D => {
            CTRL_HELD.store(true, Ordering::Relaxed);
            return None;
        }
        0x9D => {
            CTRL_HELD.store(false, Ordering::Relaxed);
            return None;
        }
        _ => {}
    }

    if byte == 0xE0 {
        EXTENDED.store(true, Ordering::Relaxed);
        return None;
    }
    let ext = EXTENDED.swap(false, Ordering::Relaxed);

    // Break codes (key release)
    if byte >= 0x80 {
        return None;
    }

    let ctrl = CTRL_HELD.load(Ordering::Relaxed);

    if ext {
        match byte {
            0x4B => Some(Key::ArrowLeft),
            0x4D => Some(Key::ArrowRight),
            0x48 => Some(Key::ArrowUp),
            0x50 => Some(Key::ArrowDown),
            0x49 => Some(Key::PageUp),
            0x51 => Some(Key::PageDown),
            0x47 => Some(Key::Home),
            0x4F => Some(Key::End),
            0x53 => Some(Key::Delete),
            0x1C => Some(Key::Enter),
            _ => Some(Key::Unknown(byte)),
        }
    } else {
        match byte {
            0x4B => Some(Key::ArrowLeft),
            0x4D => Some(Key::ArrowRight),
            0x48 => Some(Key::ArrowUp),
            0x50 => Some(Key::ArrowDown),
            0x49 => Some(Key::PageUp),
            0x51 => Some(Key::PageDown),
            0x47 => Some(Key::Home),
            0x4F => Some(Key::End),
            0x53 => Some(Key::Delete),
            0x1C => Some(Key::Enter),
            0x01 => Some(Key::Escape),
            0x0F => Some(Key::Tab),
            0x0E => Some(Key::Backspace),
            0x02 => Some(if ctrl {
                Key::Ctrl(b'1')
            } else {
                Key::Char(shifted(b'1', b'!'))
            }),
            0x03 => Some(if ctrl {
                Key::Ctrl(b'2')
            } else {
                Key::Char(shifted(b'2', b'@'))
            }),
            0x04 => Some(if ctrl {
                Key::Ctrl(b'3')
            } else {
                Key::Char(shifted(b'3', b'#'))
            }),
            0x05 => Some(if ctrl {
                Key::Ctrl(b'4')
            } else {
                Key::Char(shifted(b'4', b'$'))
            }),
            0x06 => Some(if ctrl {
                Key::Ctrl(b'5')
            } else {
                Key::Char(shifted(b'5', b'%'))
            }),
            0x07 => Some(if ctrl {
                Key::Ctrl(b'6')
            } else {
                Key::Char(shifted(b'6', b'^'))
            }),
            0x08 => Some(if ctrl {
                Key::Ctrl(b'7')
            } else {
                Key::Char(shifted(b'7', b'&'))
            }),
            0x09 => Some(if ctrl {
                Key::Ctrl(b'8')
            } else {
                Key::Char(shifted(b'8', b'*'))
            }),
            0x0A => Some(if ctrl {
                Key::Ctrl(b'9')
            } else {
                Key::Char(shifted(b'9', b'('))
            }),
            0x0B => Some(if ctrl {
                Key::Ctrl(b'0')
            } else {
                Key::Char(shifted(b'0', b')'))
            }),
            0x0C => Some(Key::Char(shifted(b'-', b'_'))),
            0x0D => Some(Key::Char(shifted(b'=', b'+'))),
            0x10 => Some(if ctrl {
                Key::Ctrl(b'q')
            } else {
                Key::Char(shifted(b'q', b'Q'))
            }),
            0x11 => Some(if ctrl {
                Key::Ctrl(b'w')
            } else {
                Key::Char(shifted(b'w', b'W'))
            }),
            0x12 => Some(if ctrl {
                Key::Ctrl(b'e')
            } else {
                Key::Char(shifted(b'e', b'E'))
            }),
            0x13 => Some(if ctrl {
                Key::Ctrl(b'r')
            } else {
                Key::Char(shifted(b'r', b'R'))
            }),
            0x14 => Some(if ctrl {
                Key::Ctrl(b't')
            } else {
                Key::Char(shifted(b't', b'T'))
            }),
            0x15 => Some(if ctrl {
                Key::Ctrl(b'y')
            } else {
                Key::Char(shifted(b'y', b'Y'))
            }),
            0x16 => Some(if ctrl {
                Key::Ctrl(b'u')
            } else {
                Key::Char(shifted(b'u', b'U'))
            }),
            0x17 => Some(if ctrl {
                Key::Ctrl(b'i')
            } else {
                Key::Char(shifted(b'i', b'I'))
            }),
            0x18 => Some(if ctrl {
                Key::Ctrl(b'o')
            } else {
                Key::Char(shifted(b'o', b'O'))
            }),
            0x19 => Some(if ctrl {
                Key::Ctrl(b'p')
            } else {
                Key::Char(shifted(b'p', b'P'))
            }),
            0x1A => Some(Key::Char(shifted(b'[', b'{'))),
            0x1B => Some(Key::Char(shifted(b']', b'}'))),
            0x1E => Some(if ctrl {
                Key::Ctrl(b'a')
            } else {
                Key::Char(shifted(b'a', b'A'))
            }),
            0x1F => Some(if ctrl {
                Key::Ctrl(b's')
            } else {
                Key::Char(shifted(b's', b'S'))
            }),
            0x20 => Some(if ctrl {
                Key::Ctrl(b'd')
            } else {
                Key::Char(shifted(b'd', b'D'))
            }),
            0x21 => Some(if ctrl {
                Key::Ctrl(b'f')
            } else {
                Key::Char(shifted(b'f', b'F'))
            }),
            0x22 => Some(if ctrl {
                Key::Ctrl(b'g')
            } else {
                Key::Char(shifted(b'g', b'G'))
            }),
            0x23 => Some(if ctrl {
                Key::Ctrl(b'h')
            } else {
                Key::Char(shifted(b'h', b'H'))
            }),
            0x24 => Some(if ctrl {
                Key::Ctrl(b'j')
            } else {
                Key::Char(shifted(b'j', b'J'))
            }),
            0x25 => Some(if ctrl {
                Key::Ctrl(b'k')
            } else {
                Key::Char(shifted(b'k', b'K'))
            }),
            0x26 => Some(if ctrl {
                Key::Ctrl(b'l')
            } else {
                Key::Char(shifted(b'l', b'L'))
            }),
            0x27 => Some(Key::Char(shifted(b';', b':'))),
            0x28 => Some(Key::Char(shifted(b'\'', b'"'))),
            0x29 => Some(Key::Char(shifted(b'`', b'~'))),
            0x2B => Some(Key::Char(shifted(b'\\', b'|'))),
            0x2C => Some(if ctrl {
                Key::Ctrl(b'z')
            } else {
                Key::Char(shifted(b'z', b'Z'))
            }),
            0x2D => Some(if ctrl {
                Key::Ctrl(b'x')
            } else {
                Key::Char(shifted(b'x', b'X'))
            }),
            0x2E => Some(if ctrl {
                Key::Ctrl(b'c')
            } else {
                Key::Char(shifted(b'c', b'C'))
            }),
            0x2F => Some(if ctrl {
                Key::Ctrl(b'v')
            } else {
                Key::Char(shifted(b'v', b'V'))
            }),
            0x30 => Some(if ctrl {
                Key::Ctrl(b'b')
            } else {
                Key::Char(shifted(b'b', b'B'))
            }),
            0x31 => Some(if ctrl {
                Key::Ctrl(b'n')
            } else {
                Key::Char(shifted(b'n', b'N'))
            }),
            0x32 => Some(if ctrl {
                Key::Ctrl(b'm')
            } else {
                Key::Char(shifted(b'm', b'M'))
            }),
            0x33 => Some(Key::Char(shifted(b',', b'<'))),
            0x34 => Some(Key::Char(shifted(b'.', b'>'))),
            0x35 => Some(Key::Char(shifted(b'/', b'?'))),
            0x39 => Some(Key::Char(b' ')),
            _ => Some(Key::Unknown(byte)),
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Fill `buf` with pending input events.  Returns how many were written.
pub fn poll_events(buf: &mut [Event]) -> usize {
    let mut count = 0;

    // ── Keyboard ──────────────────────────────────────────────────────────
    while count < buf.len() {
        let Some(scancode) = keyboard::read_scancode() else {
            break;
        };
        if let Some(key) = decode_scancode(scancode) {
            buf[count] = Event::KeyPress(key);
            count += 1;
        }
    }

    // ── Mouse ─────────────────────────────────────────────────────────────
    mouse::poll_aux_bytes();

    let (sw, sh) = crate::framebuffer::dimensions().unwrap_or((1024, 768));

    while count < buf.len() {
        let Some(pkt) = mouse::read_mouse_packet() else {
            break;
        };

        let new_x = (MOUSE_X.load(Ordering::Relaxed) + pkt.dx).clamp(0, sw as i32 - 1);
        let new_y = (MOUSE_Y.load(Ordering::Relaxed) + pkt.dy).clamp(0, sh as i32 - 1);
        MOUSE_X.store(new_x, Ordering::Relaxed);
        MOUSE_Y.store(new_y, Ordering::Relaxed);

        buf[count] = Event::MouseMove(new_x, new_y);
        count += 1;

        if count < buf.len() {
            let prev = MOUSE_BUTTONS.swap(pkt.buttons, Ordering::Relaxed);
            if prev != pkt.buttons {
                buf[count] = Event::MouseButton(pkt.buttons);
                count += 1;
            }
        }

        // Emit scroll event when wheel moves
        if count < buf.len() && pkt.scroll != 0 {
            // scroll<0 means wheel rolled toward user (down), invert so
            // positive = scroll content up (towards older lines)
            buf[count] = Event::MouseScroll(-(pkt.scroll as i32));
            count += 1;
        }
    }

    count
}
