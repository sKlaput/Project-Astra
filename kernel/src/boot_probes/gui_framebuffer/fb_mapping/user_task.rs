use super::*;

// Keep deep-probe pages in a dedicated high user range to avoid overlap with
// ELF demo ranges and other probe/task virtual regions.
const USER_FB_TASK_CODE_VIRT: usize = 0x0000_4000_8000_0000;
const USER_FB_TASK_STACK_VIRT: usize = 0x0000_4000_8001_0000;
const USER_FB_TASK_SHARED_VIRT: usize = 0x0000_4000_8002_0000;
const USER_FB_TASK_TRAP_RIP_OFFSET: u64 = 41;

pub(crate) fn probe_gui_fb_mapping_user_task() {
    let code_frame = memory::frame_allocator::allocate_frame();
    let stack_frame = memory::frame_allocator::allocate_frame();
    let shared_frame = memory::frame_allocator::allocate_frame();

    let mut map_code = false;
    let mut map_stack = false;
    let mut map_shared = false;

    if let Some(frame) = code_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_code = unsafe {
            memory::paging::map_page_current(USER_FB_TASK_CODE_VIRT, frame.start_address(), flags)
                .is_ok()
        };
    }
    if let Some(frame) = stack_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_stack = unsafe {
            memory::paging::map_page_current(USER_FB_TASK_STACK_VIRT, frame.start_address(), flags)
                .is_ok()
        };
    }
    if let Some(frame) = shared_frame {
        let flags = memory::paging::PageTableFlags::new(
            memory::paging::PageTableFlags::PRESENT
                | memory::paging::PageTableFlags::WRITABLE
                | memory::paging::PageTableFlags::USER_ACCESSIBLE,
        );
        map_shared = unsafe {
            memory::paging::map_page_current(USER_FB_TASK_SHARED_VIRT, frame.start_address(), flags)
                .is_ok()
        };
    }

    let mut done = false;
    let mut ret = 0u64;
    let mut virt = 0u64;
    let mut bytes = 0u64;
    let mut hit = false;
    let mut cs = 0u64;
    let mut rip = 0u64;
    let mut spawn = false;
    let mut exited = false;
    let mut timed_out = false;
    let mut loop_ticks = 0u64;

    if let (Some(_code_frame), Some(_stack_frame), Some(_shared_frame)) =
        (code_frame, stack_frame, shared_frame)
    {
        if map_code && map_stack && map_shared {
            let code_bytes = unsafe {
                core::slice::from_raw_parts_mut(
                    USER_FB_TASK_CODE_VIRT as *mut u8,
                    memory::paging::PAGE_SIZE,
                )
            };
            let shared_ptr = USER_FB_TASK_SHARED_VIRT as *mut u64;

            unsafe {
                core::ptr::write_bytes(code_bytes.as_mut_ptr(), 0, code_bytes.len());
                core::ptr::write_volatile(shared_ptr, 0);
                core::ptr::write_volatile(shared_ptr.add(1), 0);
                core::ptr::write_volatile(shared_ptr.add(2), 0);
            }

            // mov rbx, USER_FB_TASK_SHARED_VIRT
            code_bytes[0] = 0x48;
            code_bytes[1] = 0xBB;
            code_bytes[2..10].copy_from_slice(&(USER_FB_TASK_SHARED_VIRT as u64).to_le_bytes());
            // mov rax, SYS_MAP_FB (28)
            code_bytes[10] = 0x48;
            code_bytes[11] = 0xC7;
            code_bytes[12] = 0xC0;
            code_bytes[13] = 0x1C;
            code_bytes[14] = 0x00;
            code_bytes[15] = 0x00;
            code_bytes[16] = 0x00;
            // lea rdi, [rbx+8]   ; out[0]=virt, out[1]=bytes
            code_bytes[17] = 0x48;
            code_bytes[18] = 0x8D;
            code_bytes[19] = 0x7B;
            code_bytes[20] = 0x08;
            // clear remaining args
            code_bytes[21] = 0x48;
            code_bytes[22] = 0x31;
            code_bytes[23] = 0xF6; // xor rsi,rsi
            code_bytes[24] = 0x31;
            code_bytes[25] = 0xD2; // xor edx,edx
            code_bytes[26] = 0x45;
            code_bytes[27] = 0x31;
            code_bytes[28] = 0xD2; // xor r10d,r10d
            code_bytes[29] = 0x45;
            code_bytes[30] = 0x31;
            code_bytes[31] = 0xC0; // xor r8d,r8d
            code_bytes[32] = 0x45;
            code_bytes[33] = 0x31;
            code_bytes[34] = 0xC9; // xor r9d,r9d
                                   // syscall
            code_bytes[35] = 0x0F;
            code_bytes[36] = 0x05;
            // mov [rbx], rax  ; return code
            code_bytes[37] = 0x48;
            code_bytes[38] = 0x89;
            code_bytes[39] = 0x03;
            // int3 + jmp $
            code_bytes[40] = 0xCC;
            code_bytes[41] = 0xEB;
            code_bytes[42] = 0xFE;

            arch::x86_64::interrupts::arm_ring3_breakpoint_probe();

            let user_rsp = USER_FB_TASK_STACK_VIRT as u64 + memory::paging::PAGE_SIZE as u64 - 8;
            let task_id = scheduler::spawn_user_task_prio_name(
                USER_FB_TASK_CODE_VIRT as u64,
                USER_FB_TASK_STACK_VIRT as u64,
                USER_FB_TASK_CODE_VIRT as u64,
                user_rsp,
                20,
                "fb-map-user",
            );
            spawn = task_id.is_some();

            if let Some(task_id) = task_id {
                let start = scheduler::ticks();
                while scheduler::ticks().saturating_sub(start) < 160 {
                    if !scheduler::dispatch_once() {
                        idle::sleep_for_ticks(1);
                    }
                    loop_ticks = scheduler::ticks().saturating_sub(start);
                    ret = unsafe { core::ptr::read_volatile(shared_ptr) };
                    virt = unsafe { core::ptr::read_volatile(shared_ptr.add(1)) };
                    bytes = unsafe { core::ptr::read_volatile(shared_ptr.add(2)) };
                    if ret == 1 {
                        done = true;
                        break;
                    }
                    if scheduler::task_state(task_id) == scheduler::TaskState::Empty {
                        exited = true;
                        break;
                    }
                }

                if !done && !exited {
                    timed_out = true;
                }

                hit = arch::x86_64::interrupts::ring3_breakpoint_probe_hit();
                cs = arch::x86_64::interrupts::ring3_breakpoint_probe_cs();
                rip = arch::x86_64::interrupts::ring3_breakpoint_probe_rip();
                scheduler::exit_task(task_id);
            }
        }
    }

    let leaf_entry = if virt != 0 {
        unsafe { memory::paging::lookup_page_entry_current(virt as usize) }.unwrap_or(0)
    } else {
        0
    };
    let user_flag = (leaf_entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0;
    let write_flag = (leaf_entry & memory::paging::PageTableFlags::WRITABLE) != 0;

    serial::write_str("gui: fb-map-user map=");
    serial::write_u64(map_code as u64);
    serial::write_str(",");
    serial::write_u64(map_stack as u64);
    serial::write_str(",");
    serial::write_u64(map_shared as u64);
    serial::write_str(" spawn=");
    serial::write_u64(spawn as u64);
    serial::write_str(" done=");
    serial::write_u64(done as u64);
    serial::write_str(" exited=");
    serial::write_u64(exited as u64);
    serial::write_str(" timeout=");
    serial::write_u64(timed_out as u64);
    serial::write_str(" ticks=");
    serial::write_u64(loop_ticks);
    serial::write_str(" hit=");
    serial::write_u64(hit as u64);
    serial::write_str(" cs=");
    serial::write_u64(cs);
    serial::write_str(" rip=");
    serial::write_u64(rip);
    serial::write_str(" ret=");
    serial::write_u64(ret);
    serial::write_str(" virt=");
    serial::write_u64(virt);
    serial::write_str(" bytes=");
    serial::write_u64(bytes);
    serial::write_str(" user=");
    serial::write_u64(user_flag as u64);
    serial::write_str(" write=");
    serial::write_u64(write_flag as u64);
    serial::write_str(" leaf=");
    serial::write_u64(leaf_entry);
    serial::write_line("");

    let pass = map_code
        && map_stack
        && map_shared
        && spawn
        && done
        && hit
        && (cs & 0x3) == 0x3
        && rip == USER_FB_TASK_CODE_VIRT as u64 + USER_FB_TASK_TRAP_RIP_OFFSET
        && ret == 1
        && virt == user::USER_FRAMEBUFFER_VIRT as u64
        && bytes > 0
        && user_flag
        && write_flag;
    serial::write_line(if pass {
        "gui: fb-map-user PASS"
    } else {
        "gui: fb-map-user FAIL"
    });
}
