use super::*;

static APP_FILEMGR_DONE: AtomicU64 = AtomicU64::new(0);
static APP_FILEMGR_LAUNCH_OK: AtomicU64 = AtomicU64::new(0);
static APP_FILEMGR_ROOT_OK: AtomicU64 = AtomicU64::new(0);
static APP_FILEMGR_ETC_OK: AtomicU64 = AtomicU64::new(0);

fn task_app_file_manager_v0() {
    APP_FILEMGR_LAUNCH_OK.store(1, Ordering::Relaxed);

    let root_count = fs::directory_entry_count("/").ok();
    let root_has_etc = fs::directory_contains("/", "etc").ok();
    let root_has_hello = fs::directory_contains("/", "hello.txt").ok();

    let etc_count = fs::directory_entry_count("/etc").ok();
    let etc_has_motd = fs::directory_contains("/etc", "motd").ok();

    if root_count == Some(2) && root_has_etc == Some(true) && root_has_hello == Some(true) {
        APP_FILEMGR_ROOT_OK.store(1, Ordering::Relaxed);
    }

    if etc_count == Some(1) && etc_has_motd == Some(true) {
        APP_FILEMGR_ETC_OK.store(1, Ordering::Relaxed);
    }

    serial::write_str("apps: filemgr root_count=");
    serial::write_u64(root_count.unwrap_or(u64::MAX as usize) as u64);
    serial::write_str(" etc_count=");
    serial::write_u64(etc_count.unwrap_or(u64::MAX as usize) as u64);
    serial::write_line("");

    APP_FILEMGR_DONE.store(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_app_file_manager_v0() {
    APP_FILEMGR_DONE.store(0, Ordering::Relaxed);
    APP_FILEMGR_LAUNCH_OK.store(0, Ordering::Relaxed);
    APP_FILEMGR_ROOT_OK.store(0, Ordering::Relaxed);
    APP_FILEMGR_ETC_OK.store(0, Ordering::Relaxed);

    let task_id =
        scheduler::spawn_task_with_fn_prio_name(task_app_file_manager_v0, 20, "app-filemgr-v0");
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && APP_FILEMGR_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let launch_pass =
        task_id.is_some() && done && APP_FILEMGR_LAUNCH_OK.load(Ordering::Relaxed) == 1;
    let root_pass = task_id.is_some() && done && APP_FILEMGR_ROOT_OK.load(Ordering::Relaxed) == 1;
    let etc_pass = task_id.is_some() && done && APP_FILEMGR_ETC_OK.load(Ordering::Relaxed) == 1;

    serial::write_line(if launch_pass {
        "apps: filemgr launch PASS"
    } else {
        "apps: filemgr launch FAIL"
    });

    serial::write_line(if root_pass {
        "apps: filemgr list root PASS"
    } else {
        "apps: filemgr list root FAIL"
    });

    serial::write_line(if etc_pass {
        "apps: filemgr list etc PASS"
    } else {
        "apps: filemgr list etc FAIL"
    });
}
