pub fn user_writable_range(ptr: usize, len: usize) -> bool {
    user_range_with_flags(
        ptr,
        len,
        crate::memory::paging::PageTableFlags::USER_ACCESSIBLE
            | crate::memory::paging::PageTableFlags::WRITABLE,
    )
}

pub fn user_readable_range(ptr: usize, len: usize) -> bool {
    user_range_with_flags(
        ptr,
        len,
        crate::memory::paging::PageTableFlags::USER_ACCESSIBLE,
    )
}

fn user_range_with_flags(ptr: usize, len: usize, need: u64) -> bool {
    if len == 0 {
        return false;
    }
    if !crate::memory::paging::is_user_range(ptr, len) {
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
        let Some(entry) = (unsafe { crate::memory::paging::lookup_page_entry_current(page) })
        else {
            return false;
        };
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
