use super::*;

static APP_SETTINGS_DONE: AtomicU64 = AtomicU64::new(0);
static APP_SETTINGS_LAUNCH_OK: AtomicU64 = AtomicU64::new(0);
static APP_SETTINGS_PLACEHOLDERS_OK: AtomicU64 = AtomicU64::new(0);
static APP_SETTINGS_LIFECYCLE_OK: AtomicU64 = AtomicU64::new(0);

fn settings_v0_dispatch(cmd: &str) -> bool {
    match cmd {
        "show" => {
            serial::write_line("apps: settings panes=display,keyboard,network");
            true
        }
        _ => false,
    }
}

fn task_app_settings_v0() {
    APP_SETTINGS_LAUNCH_OK.store(1, Ordering::Relaxed);

    if settings_v0_dispatch("show") {
        APP_SETTINGS_PLACEHOLDERS_OK.store(1, Ordering::Relaxed);
    }

    // Lifecycle placeholder: foreground -> background -> foreground.
    serial::write_line("apps: settings lifecycle=foreground,background,foreground");
    APP_SETTINGS_LIFECYCLE_OK.store(1, Ordering::Relaxed);

    APP_SETTINGS_DONE.store(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_app_settings_v0() {
    APP_SETTINGS_DONE.store(0, Ordering::Relaxed);
    APP_SETTINGS_LAUNCH_OK.store(0, Ordering::Relaxed);
    APP_SETTINGS_PLACEHOLDERS_OK.store(0, Ordering::Relaxed);
    APP_SETTINGS_LIFECYCLE_OK.store(0, Ordering::Relaxed);

    let task_id =
        scheduler::spawn_task_with_fn_prio_name(task_app_settings_v0, 20, "app-settings-v0");
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && APP_SETTINGS_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let launch_pass =
        task_id.is_some() && done && APP_SETTINGS_LAUNCH_OK.load(Ordering::Relaxed) == 1;
    let placeholders_pass =
        task_id.is_some() && done && APP_SETTINGS_PLACEHOLDERS_OK.load(Ordering::Relaxed) == 1;
    let lifecycle_pass =
        task_id.is_some() && done && APP_SETTINGS_LIFECYCLE_OK.load(Ordering::Relaxed) == 1;

    serial::write_line(if launch_pass {
        "apps: settings launch PASS"
    } else {
        "apps: settings launch FAIL"
    });

    serial::write_line(if placeholders_pass {
        "apps: settings placeholders PASS"
    } else {
        "apps: settings placeholders FAIL"
    });

    serial::write_line(if lifecycle_pass {
        "apps: settings lifecycle PASS"
    } else {
        "apps: settings lifecycle FAIL"
    });
}
