use super::*;

pub(crate) fn probe_ring3_descriptors() {
    // Validate ring-3 GDT descriptors are present and have correct privilege bits.
    let code_sel = arch::x86_64::ring3_code_selector();
    let data_sel = arch::x86_64::ring3_data_selector();

    // Selectors should be non-zero.
    let code_valid = code_sel.0 != 0;
    let data_valid = data_sel.0 != 0;

    // RPL bits (bit 1-0) should be 3 for ring-3 ring privilege level.
    let code_rpl = (code_sel.0 & 0x3) as u8;
    let data_rpl = (data_sel.0 & 0x3) as u8;
    let code_rpl_ok = code_rpl == 3;
    let data_rpl_ok = data_rpl == 3;

    serial::write_str("arch: ring3-descriptors code_sel=");
    serial::write_u64(code_sel.0 as u64);
    serial::write_str(" data_sel=");
    serial::write_u64(data_sel.0 as u64);
    serial::write_str(" code_valid=");
    serial::write_u64(code_valid as u64);
    serial::write_str(" data_valid=");
    serial::write_u64(data_valid as u64);
    serial::write_str(" code_rpl=");
    serial::write_u64(code_rpl as u64);
    serial::write_str(" data_rpl=");
    serial::write_u64(data_rpl as u64);
    serial::write_line("");

    let pass = code_valid && data_valid && code_rpl_ok && data_rpl_ok;
    serial::write_line(if pass {
        "arch: ring3-descriptors PASS"
    } else {
        "arch: ring3-descriptors FAIL"
    });
}

pub(crate) fn probe_syscall_entry_msrs() {
    let kernel_cs = arch::x86_64::kernel_code_selector().0 as u64;
    let kernel_ss = arch::x86_64::kernel_data_selector().0 as u64;
    let user_cs = arch::x86_64::ring3_code_selector().0 as u64;
    let user_ss = arch::x86_64::ring3_data_selector().0 as u64;

    let efer = arch::x86_64::sysentry::efer();
    let star = arch::x86_64::sysentry::star();
    let lstar = arch::x86_64::sysentry::lstar();
    let fmask = arch::x86_64::sysentry::fmask();
    let stub = arch::x86_64::sysentry::syscall_entry_addr();

    let efer_sce = (efer & 1) != 0;
    let star_kernel = (star >> 32) & 0xffff;
    let star_user_base = (star >> 48) & 0xffff;
    let sysret_ss = star_user_base + 8;
    let sysret_cs = star_user_base + 16;
    let fmask_if = (fmask & (1 << 9)) != 0;

    serial::write_str("arch: syscall-msr efer_sce=");
    serial::write_u64(efer_sce as u64);
    serial::write_str(" kcs=");
    serial::write_u64(star_kernel);
    serial::write_str(" kss=");
    serial::write_u64(kernel_ss);
    serial::write_str(" ucs=");
    serial::write_u64(sysret_cs);
    serial::write_str(" uss=");
    serial::write_u64(sysret_ss);
    serial::write_str(" lstar_ok=");
    serial::write_u64((lstar == stub) as u64);
    serial::write_str(" fmask=");
    serial::write_u64(fmask);
    serial::write_str(" fmask_if=");
    serial::write_u64(fmask_if as u64);
    serial::write_line("");

    let pass = efer_sce
        && star_kernel == kernel_cs
        && kernel_ss == kernel_cs + 8
        && sysret_cs == user_cs
        && sysret_ss == user_ss
        && lstar == stub
        && fmask == (1 << 9)
        && fmask_if;
    serial::write_line(if pass {
        "arch: syscall-msr PASS"
    } else {
        "arch: syscall-msr FAIL"
    });
}

pub(crate) fn probe_ring3_user_mapping() {
    const USER_CODE_VIRT: usize = 0x0000_0000_0040_0000;
    const USER_STACK_VIRT: usize = 0x0000_0000_0040_1000;
    const USER_SHARED_VIRT: usize = 0x0000_0000_0040_2000;

    let code_frame = memory::frame_allocator::allocate_frame();
    let stack_frame = memory::frame_allocator::allocate_frame();
    let shared_frame = memory::frame_allocator::allocate_frame();

    let mut map_code = false;
    let mut map_stack = false;
    let mut map_shared = false;

    if let Some(frame) = code_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_code = unsafe {
            memory::paging::map_page_current(USER_CODE_VIRT, frame.start_address(), flags).is_ok()
        };
    }

    if let Some(frame) = stack_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_stack = unsafe {
            memory::paging::map_page_current(USER_STACK_VIRT, frame.start_address(), flags).is_ok()
        };
    }

    if let Some(frame) = shared_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_shared = unsafe {
            memory::paging::map_page_current(USER_SHARED_VIRT, frame.start_address(), flags).is_ok()
        };
    }

    let code_entry =
        unsafe { memory::paging::lookup_page_entry_current(USER_CODE_VIRT).unwrap_or(0) };
    let stack_entry =
        unsafe { memory::paging::lookup_page_entry_current(USER_STACK_VIRT).unwrap_or(0) };
    let shared_entry =
        unsafe { memory::paging::lookup_page_entry_current(USER_SHARED_VIRT).unwrap_or(0) };

    let code_user = (code_entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0;
    let code_write = (code_entry & memory::paging::PageTableFlags::WRITABLE) != 0;
    let stack_user = (stack_entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0;
    let stack_write = (stack_entry & memory::paging::PageTableFlags::WRITABLE) != 0;
    let shared_user = (shared_entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0;
    let shared_write = (shared_entry & memory::paging::PageTableFlags::WRITABLE) != 0;

    serial::write_str("arch: ring3-map code=");
    serial::write_u64(map_code as u64);
    serial::write_str(",");
    serial::write_u64(code_user as u64);
    serial::write_str(",");
    serial::write_u64(code_write as u64);
    serial::write_str(" stack=");
    serial::write_u64(map_stack as u64);
    serial::write_str(",");
    serial::write_u64(stack_user as u64);
    serial::write_str(",");
    serial::write_u64(stack_write as u64);
    serial::write_str(" shared=");
    serial::write_u64(map_shared as u64);
    serial::write_str(",");
    serial::write_u64(shared_user as u64);
    serial::write_str(",");
    serial::write_u64(shared_write as u64);
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && code_user
        && !code_write
        && stack_user
        && stack_write
        && shared_user
        && shared_write;
    serial::write_line(if pass {
        "arch: ring3-map PASS"
    } else {
        "arch: ring3-map FAIL"
    });
}
