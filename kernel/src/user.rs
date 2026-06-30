/// User-mode task infrastructure.
///
/// Provides support for spawning and managing persistent user-mode tasks
/// that can be scheduled and rescheduled by the kernel scheduler.
use crate::memory::paging::{
    hhdm_offset, lookup_page_entry_current, map_page_current, PageTableFlags, PAGE_SIZE,
};

/// Virtual address for user framebuffer window.
pub const USER_FRAMEBUFFER_VIRT: usize = 0x0000_4000_2000_0000;

/// User task stack virtual address.
pub const USER_TASK_STACK_VIRT: usize = 0x0000_4000_1000_0000;

/// Map the boot framebuffer into the current user address space at
/// USER_FRAMEBUFFER_VIRT, page-aligned, user-accessible, writable.
///
/// Returns (mapped_virtual_base, framebuffer_byte_len) on success.
pub fn map_framebuffer_for_current_task() -> Option<(u64, u64)> {
    // Keep mapping scoped to active user-task context.
    let current = match crate::scheduler::current_task() {
        Some(id) => id,
        None => return None,
    };
    if !crate::scheduler::is_user_task(current) {
        return None;
    }

    let info = crate::boot::protocol::framebuffer_info()?;
    if info.addr.is_null() || info.bpp != 32 || info.pitch < info.width.saturating_mul(4) {
        return None;
    }

    let fb_bytes = (info.pitch as usize).saturating_mul(info.height as usize);
    if fb_bytes == 0 {
        return None;
    }

    let page_count = (fb_bytes + PAGE_SIZE - 1) / PAGE_SIZE;
    let fb_virt = info.addr as usize;
    let hhdm = hhdm_offset();
    if fb_virt < hhdm {
        return None;
    }
    let flags = PageTableFlags::new(
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
    );

    for i in 0..page_count {
        let dst = USER_FRAMEBUFFER_VIRT.saturating_add(i.saturating_mul(PAGE_SIZE));
        // If already mapped, leave it as-is to keep this idempotent.
        let already_mapped = unsafe { lookup_page_entry_current(dst).is_some() };
        if already_mapped {
            continue;
        }

        let src_virt = fb_virt.saturating_add(i.saturating_mul(PAGE_SIZE));
        if src_virt < hhdm {
            return None;
        }
        let phys = src_virt - hhdm;
        if unsafe { map_page_current(dst, phys, flags) }.is_err() {
            return None;
        }
    }

    Some((USER_FRAMEBUFFER_VIRT as u64, fb_bytes as u64))
}
