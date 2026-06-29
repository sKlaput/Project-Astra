use super::*;

static GUI_FB_MAP_DONE: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_OK: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_VIRT: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_BYTES: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_USER: AtomicU64 = AtomicU64::new(0);
static GUI_FB_MAP_WRITE: AtomicU64 = AtomicU64::new(0);

fn task_gui_fb_map_probe() {
    if !GUI_FB_DEEP_PROBE {
        // Safe smoke probe: invoke SYS_MAP_FB with out_ptr=0 and validate it
        // fails cleanly (return 0) without touching caller memory.
        let ret = syscall::dispatch(syscall::SYS_MAP_FB, 0, 0, 0, 0, 0, 0);

        GUI_FB_MAP_OK.store((ret == 0) as u64, Ordering::Relaxed);
        GUI_FB_MAP_VIRT.store(0, Ordering::Relaxed);
        GUI_FB_MAP_BYTES.store(ret, Ordering::Relaxed);
        GUI_FB_MAP_USER.store(0, Ordering::Relaxed);
        GUI_FB_MAP_WRITE.store(0, Ordering::Relaxed);
    } else {
        // Deeper probe: provide an output buffer, then verify mapping metadata.
        let mut out = [0u64; 2];
        let out_ptr = out.as_mut_ptr() as usize as u64;
        let ret = syscall::dispatch(syscall::SYS_MAP_FB, out_ptr, 0, 0, 0, 0, 0);

        // This probe task is kernel-backed. With user-only mapping policy,
        // SYS_MAP_FB must deny the request cleanly.
        let in_user_task = scheduler::current_task()
            .map(scheduler::is_user_task)
            .unwrap_or(false);
        if !in_user_task {
            GUI_FB_MAP_OK.store((ret == 0) as u64, Ordering::Relaxed);
            GUI_FB_MAP_VIRT.store(0, Ordering::Relaxed);
            GUI_FB_MAP_BYTES.store(ret, Ordering::Relaxed);
            GUI_FB_MAP_USER.store(0, Ordering::Relaxed);
            GUI_FB_MAP_WRITE.store(0, Ordering::Relaxed);
            GUI_FB_MAP_DONE.store(1, Ordering::Relaxed);

            if let Some(id) = scheduler::current_task() {
                scheduler::exit_task(id);
            }
            return;
        }

        let virt_base = out[0];
        let byte_len = out[1];
        let mut user_flag = 0u64;
        let mut write_flag = 0u64;

        if ret == 1 && virt_base != 0 {
            let entry = unsafe { memory::paging::lookup_page_entry_current(virt_base as usize) }
                .unwrap_or(0);
            user_flag = ((entry & memory::paging::PageTableFlags::USER_ACCESSIBLE) != 0) as u64;
            write_flag = ((entry & memory::paging::PageTableFlags::WRITABLE) != 0) as u64;
        }

        let ok = ret == 1
            && virt_base == user::USER_FRAMEBUFFER_VIRT as u64
            && byte_len > 0
            && user_flag == 1
            && write_flag == 1;
        GUI_FB_MAP_OK.store(ok as u64, Ordering::Relaxed);
        GUI_FB_MAP_VIRT.store(virt_base, Ordering::Relaxed);
        GUI_FB_MAP_BYTES.store(byte_len, Ordering::Relaxed);
        GUI_FB_MAP_USER.store(user_flag, Ordering::Relaxed);
        GUI_FB_MAP_WRITE.store(write_flag, Ordering::Relaxed);
    }

    GUI_FB_MAP_DONE.store(1, Ordering::Relaxed);

    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_gui_fb_mapping() {
    GUI_FB_MAP_DONE.store(0, Ordering::Relaxed);
    GUI_FB_MAP_OK.store(0, Ordering::Relaxed);
    GUI_FB_MAP_VIRT.store(0, Ordering::Relaxed);
    GUI_FB_MAP_BYTES.store(0, Ordering::Relaxed);
    GUI_FB_MAP_USER.store(0, Ordering::Relaxed);
    GUI_FB_MAP_WRITE.store(0, Ordering::Relaxed);

    let task_id = scheduler::spawn_task_with_fn(task_gui_fb_map_probe);
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && GUI_FB_MAP_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let ok = GUI_FB_MAP_OK.load(Ordering::Relaxed);
    let virt = GUI_FB_MAP_VIRT.load(Ordering::Relaxed);
    let ret = GUI_FB_MAP_BYTES.load(Ordering::Relaxed);
    let user = GUI_FB_MAP_USER.load(Ordering::Relaxed);
    let write = GUI_FB_MAP_WRITE.load(Ordering::Relaxed);

    serial::write_str("gui: fb-map spawn=");
    serial::write_u64(task_id.is_some() as u64);
    serial::write_str(" done=");
    serial::write_u64(done as u64);
    serial::write_str(" map=");
    serial::write_u64(ok);
    serial::write_str(" virt=");
    serial::write_u64(virt);
    serial::write_str(" ret=");
    serial::write_u64(ret);
    serial::write_str(" user=");
    serial::write_u64(user);
    serial::write_str(" write=");
    serial::write_u64(write);
    serial::write_str(" deep=");
    serial::write_u64(GUI_FB_DEEP_PROBE as u64);
    serial::write_line("");

    let pass = if !GUI_FB_DEEP_PROBE {
        task_id.is_some() && done && ok == 1 && virt == 0 && ret == 0 && user == 0 && write == 0
    } else {
        // Deep kernel-mode probe now validates a clean deny path because
        // SYS_MAP_FB is intentionally scoped to user tasks.
        task_id.is_some() && done && ok == 1 && virt == 0 && ret == 0 && user == 0 && write == 0
    };
    serial::write_line(if pass {
        "gui: fb-map PASS"
    } else {
        "gui: fb-map FAIL"
    });
}
