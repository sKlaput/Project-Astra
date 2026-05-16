// ---------------------------------------------------------------------------
// Internal syscall dispatch table (kernel-side scaffold).
// ---------------------------------------------------------------------------

use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};

/// Unknown syscall sentinel.
pub const SYS_ENOSYS: u64 = u64::MAX;

/// Syscall numbers for the internal probe ABI.
pub const SYS_NOP: u64 = 0;
pub const SYS_ADD: u64 = 1;
pub const SYS_MAX: u64 = 2;
pub const SYS_XORROT: u64 = 3;
pub const SYS_TICKS: u64 = 4;
pub const SYS_TASK_ID: u64 = 5;
pub const SYS_SIGNAL_SET: u64 = 6;
pub const SYS_SIGNAL_PENDING: u64 = 7;
pub const SYS_SIGNAL_CLEAR: u64 = 8;
pub const SYS_SIGNAL_WAIT_UNTIL: u64 = 9;
pub const SYS_SIGNAL_WAIT: u64 = 10;
pub const SYS_SIGNAL_WAIT_ALL_UNTIL: u64 = 11;
pub const SYS_SIGNAL_MASK_GET: u64 = 12;
pub const SYS_SIGNAL_BLOCK: u64 = 13;
pub const SYS_SIGNAL_UNBLOCK: u64 = 14;
pub const SYS_SIGNAL_WAIT_CONSUME_UNTIL: u64 = 15;
pub const SYS_SIGNAL_WAIT_CONSUME: u64 = 16;
pub const SYS_SIGNAL_WAIT_ALL_CONSUME_UNTIL: u64 = 17;
pub const SYS_SIGNAL_WAIT_ALL_CONSUME: u64 = 18;

/// Write printable ASCII bytes (and `\n`) to the kernel serial console.
/// a=ptr (virtual address), b=byte length (capped at 512).
/// Returns 0 on success, 1 on bad arguments.
pub const SYS_WRITE_CONSOLE: u64 = 19;

/// Yield the current task for one scheduler tick.
pub const SYS_YIELD: u64 = 20;

/// Exit the current task.  Does not return to the caller.
pub const SYS_EXIT: u64 = 21;

/// IPC: Send a message to another process (or system-wide).
/// a=message_ptr (64 bytes max), b=message_len (0-64)
/// Returns 1 on success, 0 on error.
pub const SYS_SEND_MSG: u64 = 22;

/// IPC: Try to receive a message (non-blocking).
/// a=buffer_ptr (must be 64+ bytes), b=unused
/// Returns number of bytes received (0 if no message waiting), or u64::MAX on error.
pub const SYS_RECV_MSG: u64 = 23;

/// Graphics: Get framebuffer information.
/// a=info_ptr (must point to 32+ bytes for info struct)
/// Returns 1 if framebuffer available, 0 otherwise.
/// Info struct layout (at a): [width:u32][height:u32][pitch:u32][bpp:u8][padding:7 bytes]
pub const SYS_GET_FB_INFO: u64 = 24;

/// Graphics: Draw a filled rectangle.
/// a=x (u32), b=y (u32), c=width (u32), d=height (u32), e=color (0xRRGGBB)
/// Returns 1 on success, 0 on error.
pub const SYS_DRAW_RECT: u64 = 25;

/// Graphics: Draw a single pixel.
/// a=x (u32), b=y (u32), c=color (0xRRGGBB)
/// Returns 1 on success, 0 on error.
pub const SYS_DRAW_PIXEL: u64 = 26;

/// Graphics: Draw text at coordinates.
/// a=text_ptr, b=text_len (1..=256), c=x, d=y, e=color (0xRRGGBB)
/// Returns 1 on success, 0 on error.
pub const SYS_DRAW_TEXT: u64 = 27;

/// Graphics: Map framebuffer into current user task address space.
/// a=out_ptr to [u64;2] where kernel writes [mapped_user_virt, byte_len]
/// Returns 1 on success, 0 on error.
pub const SYS_MAP_FB: u64 = 28;

// Simple kernel IPC message buffer: holds one pending message.
// In a real system this would be a queue per process, but for proof-of-concept this is a global buffer.
struct PendingMessage {
    sender_task: u64,
    len: u64,
    data: [u8; 64],
}

impl PendingMessage {
    const fn empty() -> Self {
        Self {
            sender_task: 0,
            len: 0,
            data: [0u8; 64],
        }
    }
}

static PENDING_MESSAGE: Mutex<PendingMessage> = Mutex::new(PendingMessage::empty());

type SysHandler = fn(u64, u64, u64, u64, u64, u64) -> u64;

pub const AUTHZ_REASON_ALLOW: u64 = 0;
pub const AUTHZ_REASON_DENY_UNKNOWN_SYSCALL: u64 = 1;
pub const AUTHZ_REASON_DENY_DEFAULT: u64 = 2;
pub const AUTHZ_REASON_DENY_PRIVILEGED_GROUP: u64 = 3;

#[derive(Clone, Copy, Debug)]
pub struct SecurityAuthzSnapshot {
    pub checks: u64,
    pub denied: u64,
    pub last_reason: u64,
    pub deny_unknown: u64,
    pub deny_default: u64,
    pub deny_privileged: u64,
}

static AUTHZ_CHECKS: AtomicU64 = AtomicU64::new(0);
static AUTHZ_DENIED: AtomicU64 = AtomicU64::new(0);
static AUTHZ_LAST_REASON: AtomicU64 = AtomicU64::new(AUTHZ_REASON_ALLOW);
static AUTHZ_DENY_UNKNOWN: AtomicU64 = AtomicU64::new(0);
static AUTHZ_DENY_DEFAULT: AtomicU64 = AtomicU64::new(0);
static AUTHZ_DENY_PRIVILEGED: AtomicU64 = AtomicU64::new(0);

fn authz_record(reason: u64, allowed: bool) {
    AUTHZ_CHECKS.fetch_add(1, Ordering::Relaxed);
    if !allowed {
        AUTHZ_DENIED.fetch_add(1, Ordering::Relaxed);
        match reason {
            AUTHZ_REASON_DENY_UNKNOWN_SYSCALL => {
                AUTHZ_DENY_UNKNOWN.fetch_add(1, Ordering::Relaxed);
            }
            AUTHZ_REASON_DENY_DEFAULT => {
                AUTHZ_DENY_DEFAULT.fetch_add(1, Ordering::Relaxed);
            }
            AUTHZ_REASON_DENY_PRIVILEGED_GROUP => {
                AUTHZ_DENY_PRIVILEGED.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
    AUTHZ_LAST_REASON.store(reason, Ordering::Relaxed);
}

fn is_privileged_syscall_group(nr: u64) -> bool {
    matches!(
        nr,
        SYS_SIGNAL_SET
            | SYS_SIGNAL_PENDING
            | SYS_SIGNAL_CLEAR
            | SYS_SIGNAL_WAIT_UNTIL
            | SYS_SIGNAL_WAIT
            | SYS_SIGNAL_WAIT_ALL_UNTIL
            | SYS_SIGNAL_MASK_GET
            | SYS_SIGNAL_BLOCK
            | SYS_SIGNAL_UNBLOCK
            | SYS_SIGNAL_WAIT_CONSUME_UNTIL
            | SYS_SIGNAL_WAIT_CONSUME
            | SYS_SIGNAL_WAIT_ALL_CONSUME_UNTIL
            | SYS_SIGNAL_WAIT_ALL_CONSUME
    )
}

fn authorize_syscall_for_caller(nr: u64, caller_is_user: bool) -> (bool, u64) {
    if nr >= SYSCALL_TABLE.len() as u64 {
        return (false, AUTHZ_REASON_DENY_UNKNOWN_SYSCALL);
    }

    if caller_is_user && is_privileged_syscall_group(nr) {
        return (false, AUTHZ_REASON_DENY_PRIVILEGED_GROUP);
    }

    // E13 Slice 4 baseline: deny privileged syscall groups for user callers.
    (true, AUTHZ_REASON_ALLOW)
}

fn authorize_syscall(nr: u64) -> (bool, u64) {
    let caller_is_user = crate::scheduler::current_task()
        .map(crate::scheduler::is_user_task)
        .unwrap_or(false);
    authorize_syscall_for_caller(nr, caller_is_user)
}

fn user_writable_range(ptr: usize, len: usize) -> bool {
    if len == 0 {
        return false;
    }

    let end = match ptr.checked_add(len.saturating_sub(1)) {
        Some(v) => v,
        None => return false,
    };

    let first_page = ptr & !0xFFFusize;
    let last_page = end & !0xFFFusize;
    let mut page = first_page;

    loop {
        let Some(entry) = (unsafe { crate::memory::paging::lookup_page_entry_current(page) }) else {
            return false;
        };
        let need = crate::memory::paging::PageTableFlags::USER_ACCESSIBLE
            | crate::memory::paging::PageTableFlags::WRITABLE;
        if entry & need != need {
            return false;
        }

        if page == last_page {
            break;
        }
        page = match page.checked_add(crate::memory::paging::PAGE_SIZE) {
            Some(v) => v,
            None => return false,
        };
    }

    true
}

fn sys_nop(_a: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    0
}

fn sys_add(a: u64, b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    a.wrapping_add(b)
}

fn sys_max(a: u64, b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if a > b { a } else { b }
}

fn sys_xorrot(a: u64, b: u64, c: u64, d: u64, _e: u64, _f: u64) -> u64 {
    (a ^ b).rotate_left((c & 63) as u32) ^ d
}

fn sys_ticks(_a: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::ticks()
}

fn sys_task_id(_a: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::current_task().map(|t| t.0).unwrap_or(0)
}

fn sys_signal_set(id: u64, bits: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_signal(crate::scheduler::TaskId(id), bits) as u64
}

fn sys_signal_pending(id: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_pending_signals(crate::scheduler::TaskId(id))
}

fn sys_signal_clear(id: u64, bits: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_clear_signals(crate::scheduler::TaskId(id), bits)
}

fn sys_signal_wait_until(id: u64, bits: u64, deadline_tick: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_signal_until_tick(crate::scheduler::TaskId(id), bits, deadline_tick) as u64
}

fn sys_signal_wait(id: u64, bits: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_signal(crate::scheduler::TaskId(id), bits) as u64
}

fn sys_signal_wait_all_until(id: u64, bits: u64, deadline_tick: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_all_signals_until_tick(crate::scheduler::TaskId(id), bits, deadline_tick) as u64
}

fn sys_signal_mask_get(id: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_signal_mask(crate::scheduler::TaskId(id))
}

fn sys_signal_block(id: u64, bits: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_block_signals(crate::scheduler::TaskId(id), bits)
}

fn sys_signal_unblock(id: u64, bits: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_unblock_signals(crate::scheduler::TaskId(id), bits)
}

fn sys_signal_wait_consume_until(id: u64, bits: u64, deadline_tick: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_consume_signal_until_tick(crate::scheduler::TaskId(id), bits, deadline_tick)
}

fn sys_signal_wait_consume(id: u64, bits: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_consume_signal(crate::scheduler::TaskId(id), bits)
}

fn sys_signal_wait_all_consume_until(id: u64, bits: u64, deadline_tick: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_all_consume_signals_until_tick(crate::scheduler::TaskId(id), bits, deadline_tick)
}

fn sys_signal_wait_all_consume(id: u64, bits: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_all_consume_signals(crate::scheduler::TaskId(id), bits)
}

fn sys_write_console(ptr: u64, len: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if ptr == 0 || len == 0 { return 0; }
    let byte_len = (len as usize).min(512);
    // SAFETY: single-address-space kernel; ptr is caller-supplied and non-null.
    // We only emit printable ASCII and newline, so terminal injection is not possible.
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, byte_len) };
    for &b in bytes {
        if b == b'\n' || (b >= 0x20 && b < 0x7F) {
            // Single-byte ASCII is always valid UTF-8.
            let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_ref(&b)) };
            crate::serial::write_str(s);
        }
    }
    0
}

fn sys_yield(_a: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::sleep_current_for_ticks(1);
    0
}

fn sys_exit_task(_a: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if let Some(id) = crate::scheduler::current_task() {
        crate::scheduler::exit_task(id);
    }
    // If exit_task returned (outside dispatch), return 0 so the stub can sysretq safely.
    0
}

fn sys_send_msg(ptr: u64, len: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if ptr == 0 || len == 0 || len > 64 {
        return 0;
    }
    
    // SAFETY: single-address-space kernel; ptr is caller-supplied and non-null.
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    
    let sender_id = crate::scheduler::current_task().map(|t| t.0).unwrap_or(0);
    let mut msg = PENDING_MESSAGE.lock();
    msg.sender_task = sender_id;
    msg.len = len;
    msg.data[..len as usize].copy_from_slice(bytes);
    drop(msg);
    
    1
}

fn sys_recv_msg(ptr: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if ptr == 0 {
        return u64::MAX;
    }
    
    let mut msg = PENDING_MESSAGE.lock();
    if msg.len == 0 {
        return 0; // No message waiting
    }
    
    let len = msg.len;
    // SAFETY: single-address-space kernel; ptr is caller-supplied.
    let buf = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, 64) };
    buf[..len as usize].copy_from_slice(&msg.data[..len as usize]);
    msg.len = 0; // Clear the pending message
    
    len
}

fn sys_get_fb_info(ptr: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if ptr == 0 {
        return 0;
    }
    
    if let Some(info) = crate::boot::protocol::framebuffer_info() {
        // SAFETY: single-address-space kernel; ptr is caller-supplied.
        let buf = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u32, 8) };
        buf[0] = info.width as u32;
        buf[1] = info.height as u32;
        buf[2] = info.pitch as u32;
        buf[3] = info.bpp as u32;
        1
    } else {
        0
    }
}

fn encode_channel(value: u8, size: u8, shift: u8) -> u32 {
    if size == 0 || size > 8 || shift >= 32 {
        return 0;
    }

    let Some(max_shifted) = 1u32.checked_shl(size as u32) else {
        return 0;
    };
    let max = max_shifted.saturating_sub(1);
    let normalized = (value as u32 * max) / 255;
    normalized.checked_shl(shift as u32).unwrap_or(0)
}

fn fb_color_from_rgb(info: &crate::boot::protocol::FramebufferInfo, rgb: u32) -> u32 {
    let red = ((rgb >> 16) & 0xFF) as u8;
    let green = ((rgb >> 8) & 0xFF) as u8;
    let blue = (rgb & 0xFF) as u8;

    encode_channel(red, info.red_mask_size, info.red_mask_shift)
        | encode_channel(green, info.green_mask_size, info.green_mask_shift)
        | encode_channel(blue, info.blue_mask_size, info.blue_mask_shift)
}

fn sys_draw_rect(x: u64, y: u64, w: u64, h: u64, color: u64, _f: u64) -> u64 {
    let Some(info) = crate::boot::protocol::framebuffer_info() else {
        return 0;
    };
    if info.addr.is_null() || info.bpp != 32 || info.pitch < info.width.saturating_mul(4) {
        return 0;
    }

    let x = x as usize;
    let y = y as usize;
    let w = w as usize;
    let h = h as usize;
    let fb_w = info.width as usize;
    let fb_h = info.height as usize;
    let pitch = info.pitch as usize;

    if w == 0 || h == 0 || x >= fb_w || y >= fb_h {
        return 0;
    }

    let x_end = x.saturating_add(w).min(fb_w);
    let y_end = y.saturating_add(h).min(fb_h);
    let pixel = fb_color_from_rgb(&info, color as u32);

    for py in y..y_end {
        for px in x..x_end {
            let byte_offset = py.saturating_mul(pitch).saturating_add(px.saturating_mul(4));
            // SAFETY: framebuffer geometry is validated above and coordinates are clipped.
            unsafe {
                info.addr.add(byte_offset).cast::<u32>().write_volatile(pixel);
            }
        }
    }

    1
}

fn sys_draw_pixel(x: u64, y: u64, color: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    let Some(info) = crate::boot::protocol::framebuffer_info() else {
        return 0;
    };
    if info.addr.is_null() || info.bpp != 32 || info.pitch < info.width.saturating_mul(4) {
        return 0;
    }

    let x = x as usize;
    let y = y as usize;
    let fb_w = info.width as usize;
    let fb_h = info.height as usize;
    if x >= fb_w || y >= fb_h {
        return 0;
    }

    let pixel = fb_color_from_rgb(&info, color as u32);
    let byte_offset = y
        .saturating_mul(info.pitch as usize)
        .saturating_add(x.saturating_mul(4));
    // SAFETY: framebuffer geometry is validated above and coordinates are in-bounds.
    unsafe {
        info.addr.add(byte_offset).cast::<u32>().write_volatile(pixel);
    }
    1
}

fn sys_draw_text(ptr: u64, len: u64, x: u64, y: u64, color: u64, _f: u64) -> u64 {
    if ptr == 0 || len == 0 {
        return 0;
    }
    let byte_len = (len as usize).min(256);

    // SAFETY: single-address-space kernel; ptr is caller-supplied and non-null.
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, byte_len) };

    let mut scratch = [0u8; 256];
    let mut out = 0usize;
    for &b in bytes {
        if b == b'\n' || (b >= 0x20 && b < 0x7F) {
            scratch[out] = b;
            out += 1;
        }
    }
    if out == 0 {
        return 0;
    }

    let text = unsafe { core::str::from_utf8_unchecked(&scratch[..out]) };
    if crate::framebuffer::draw_text_at(x as usize, y as usize, text, color as u32) {
        1
    } else {
        0
    }
}

fn sys_map_fb(out_ptr: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if out_ptr == 0 {
        return 0;
    }

    if !user_writable_range(out_ptr as usize, core::mem::size_of::<u64>() * 2) {
        return 0;
    }

    let Some((virt_base, byte_len)) = crate::user::map_framebuffer_for_current_task() else {
        return 0;
    };

    // SAFETY: single-address-space kernel; pointer provided by caller.
    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr as *mut u64, 2) };
    out[0] = virt_base;
    out[1] = byte_len;
    1
}

static SYSCALL_TABLE: [SysHandler; 29] = [
    sys_nop,
    sys_add,
    sys_max,
    sys_xorrot,
    sys_ticks,
    sys_task_id,
    sys_signal_set,
    sys_signal_pending,
    sys_signal_clear,
    sys_signal_wait_until,
    sys_signal_wait,
    sys_signal_wait_all_until,
    sys_signal_mask_get,
    sys_signal_block,
    sys_signal_unblock,
    sys_signal_wait_consume_until,
    sys_signal_wait_consume,
    sys_signal_wait_all_consume_until,
    sys_signal_wait_all_consume,
    sys_write_console,
    sys_yield,
    sys_exit_task,
    sys_send_msg,
    sys_recv_msg,
    sys_get_fb_info,
    sys_draw_rect,
    sys_draw_pixel,
    sys_draw_text,
    sys_map_fb,
];

/// Dispatch a syscall by number using the internal fixed table.
///
/// Argument order matches the project ABI baseline: rdi, rsi, rdx, r10, r8, r9.
pub fn dispatch(nr: u64, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> u64 {
    let (allowed, reason) = authorize_syscall(nr);
    authz_record(reason, allowed);
    if !allowed {
        return SYS_ENOSYS;
    }

    match SYSCALL_TABLE.get(nr as usize) {
        Some(handler) => handler(a, b, c, d, e, f),
        None => {
            authz_record(AUTHZ_REASON_DENY_DEFAULT, false);
            SYS_ENOSYS
        }
    }
}

/// Number of syscall slots currently implemented.
pub fn table_len() -> u64 {
    SYSCALL_TABLE.len() as u64
}

pub fn security_authz_snapshot() -> SecurityAuthzSnapshot {
    SecurityAuthzSnapshot {
        checks: AUTHZ_CHECKS.load(Ordering::Relaxed),
        denied: AUTHZ_DENIED.load(Ordering::Relaxed),
        last_reason: AUTHZ_LAST_REASON.load(Ordering::Relaxed),
        deny_unknown: AUTHZ_DENY_UNKNOWN.load(Ordering::Relaxed),
        deny_default: AUTHZ_DENY_DEFAULT.load(Ordering::Relaxed),
        deny_privileged: AUTHZ_DENY_PRIVILEGED.load(Ordering::Relaxed),
    }
}

pub fn security_probe_record_user_authz(nr: u64) -> bool {
    let (allowed, reason) = authorize_syscall_for_caller(nr, true);
    authz_record(reason, allowed);
    allowed
}
