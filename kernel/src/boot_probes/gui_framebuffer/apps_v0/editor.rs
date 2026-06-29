use super::*;

static APP_EDITOR_DONE: AtomicU64 = AtomicU64::new(0);
static APP_EDITOR_LAUNCH_OK: AtomicU64 = AtomicU64::new(0);
static APP_EDITOR_OPEN_OK: AtomicU64 = AtomicU64::new(0);
static APP_EDITOR_DISPLAY_OK: AtomicU64 = AtomicU64::new(0);

fn task_app_text_editor_v0() {
    APP_EDITOR_LAUNCH_OK.store(1, Ordering::Relaxed);

    if let Ok(mut fh) = fs::open("/etc/motd") {
        APP_EDITOR_OPEN_OK.store(1, Ordering::Relaxed);

        let mut buf = [0u8; 64];
        if let Ok(n) = fs::read(&mut fh, &mut buf) {
            let expected = b"kernel vfs motd\n";
            if n == expected.len() && &buf[..n] == expected {
                APP_EDITOR_DISPLAY_OK.store(1, Ordering::Relaxed);
            }

            serial::write_str("apps: editor file=/etc/motd bytes=");
            serial::write_u64(n as u64);
            serial::write_line("");
        }
    }

    APP_EDITOR_DONE.store(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_app_text_editor_v0() {
    APP_EDITOR_DONE.store(0, Ordering::Relaxed);
    APP_EDITOR_LAUNCH_OK.store(0, Ordering::Relaxed);
    APP_EDITOR_OPEN_OK.store(0, Ordering::Relaxed);
    APP_EDITOR_DISPLAY_OK.store(0, Ordering::Relaxed);

    let task_id =
        scheduler::spawn_task_with_fn_prio_name(task_app_text_editor_v0, 20, "app-editor-v0");
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && APP_EDITOR_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let launch_pass =
        task_id.is_some() && done && APP_EDITOR_LAUNCH_OK.load(Ordering::Relaxed) == 1;
    let open_pass = task_id.is_some() && done && APP_EDITOR_OPEN_OK.load(Ordering::Relaxed) == 1;
    let display_pass =
        task_id.is_some() && done && APP_EDITOR_DISPLAY_OK.load(Ordering::Relaxed) == 1;

    serial::write_line(if launch_pass {
        "apps: editor launch PASS"
    } else {
        "apps: editor launch FAIL"
    });

    serial::write_line(if open_pass {
        "apps: editor open PASS"
    } else {
        "apps: editor open FAIL"
    });

    serial::write_line(if display_pass {
        "apps: editor display PASS"
    } else {
        "apps: editor display FAIL"
    });
}
