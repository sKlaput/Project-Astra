use spin::Mutex;

use super::memory::{user_readable_range, user_writable_range};

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

pub type SysHandler = fn(u64, u64, u64, u64, u64, u64) -> u64;

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
    crate::scheduler::task_wait_all_signals_until_tick(
        crate::scheduler::TaskId(id),
        bits,
        deadline_tick,
    ) as u64
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
    crate::scheduler::task_wait_consume_signal_until_tick(
        crate::scheduler::TaskId(id),
        bits,
        deadline_tick,
    )
}

fn sys_signal_wait_consume(id: u64, bits: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_consume_signal(crate::scheduler::TaskId(id), bits)
}

fn sys_signal_wait_all_consume_until(id: u64, bits: u64, deadline_tick: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_all_consume_signals_until_tick(
        crate::scheduler::TaskId(id),
        bits,
        deadline_tick,
    )
}

fn sys_signal_wait_all_consume(id: u64, bits: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    crate::scheduler::task_wait_all_consume_signals(crate::scheduler::TaskId(id), bits)
}

fn sys_write_console(ptr: u64, len: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if ptr == 0 || len == 0 {
        return 0;
    }
    let byte_len = (len as usize).min(512);
    if !user_readable_range(ptr as usize, byte_len) {
        return 0;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, byte_len) };
    crate::arch::x86_64::cpu::with_user_access(|| {
        for &b in bytes {
            if b == b'\n' || (b >= 0x20 && b < 0x7F) {
                let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_ref(&b)) };
                crate::serial::write_str(s);
            }
        }
    });
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
    0
}

fn sys_send_msg(ptr: u64, len: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if ptr == 0 || len == 0 || len > 64 {
        return 0;
    }
    if !user_readable_range(ptr as usize, len as usize) {
        return 0;
    }

    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let sender_id = crate::scheduler::current_task().map(|t| t.0).unwrap_or(0);
    let mut msg = PENDING_MESSAGE.lock();
    msg.sender_task = sender_id;
    msg.len = len;
    crate::arch::x86_64::cpu::with_user_access(|| {
        msg.data[..len as usize].copy_from_slice(bytes);
    });
    drop(msg);
    1
}

fn sys_recv_msg(ptr: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if ptr == 0 {
        return u64::MAX;
    }

    let mut msg = PENDING_MESSAGE.lock();
    if msg.len == 0 {
        return 0;
    }

    let len = msg.len;
    if !user_writable_range(ptr as usize, 64) {
        return 0;
    }
    let buf = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, 64) };
    crate::arch::x86_64::cpu::with_user_access(|| {
        buf[..len as usize].copy_from_slice(&msg.data[..len as usize]);
    });
    msg.len = 0;
    len
}

fn sys_get_fb_info(ptr: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
    if ptr == 0 {
        return 0;
    }

    if let Some(info) = crate::boot::protocol::framebuffer_info() {
        if !user_writable_range(ptr as usize, core::mem::size_of::<u32>() * 8) {
            return 0;
        }
        let buf = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u32, 8) };
        crate::arch::x86_64::cpu::with_user_access(|| {
            buf[0] = info.width as u32;
            buf[1] = info.height as u32;
            buf[2] = info.pitch as u32;
            buf[3] = info.bpp as u32;
        });
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
            let byte_offset = py
                .saturating_mul(pitch)
                .saturating_add(px.saturating_mul(4));
            unsafe {
                info.addr
                    .add(byte_offset)
                    .cast::<u32>()
                    .write_volatile(pixel);
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
    unsafe {
        info.addr
            .add(byte_offset)
            .cast::<u32>()
            .write_volatile(pixel);
    }
    1
}

fn sys_draw_text(ptr: u64, len: u64, x: u64, y: u64, color: u64, _f: u64) -> u64 {
    if ptr == 0 || len == 0 {
        return 0;
    }
    let byte_len = (len as usize).min(256);
    if !user_readable_range(ptr as usize, byte_len) {
        return 0;
    }

    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, byte_len) };
    let mut scratch = [0u8; 256];
    let mut out = 0usize;
    crate::arch::x86_64::cpu::with_user_access(|| {
        for &b in bytes {
            if b == b'\n' || (b >= 0x20 && b < 0x7F) {
                scratch[out] = b;
                out += 1;
            }
        }
    });
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

    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr as *mut u64, 2) };
    crate::arch::x86_64::cpu::with_user_access(|| {
        out[0] = virt_base;
        out[1] = byte_len;
    });
    1
}

pub static SYSCALL_TABLE: [SysHandler; 29] = [
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
