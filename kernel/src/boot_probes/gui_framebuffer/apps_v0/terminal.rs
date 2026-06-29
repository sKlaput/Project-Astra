use super::*;

static APP_TERMINAL_DONE: AtomicU64 = AtomicU64::new(0);
static APP_TERMINAL_LAUNCH_OK: AtomicU64 = AtomicU64::new(0);
static APP_TERMINAL_HELP_OK: AtomicU64 = AtomicU64::new(0);

fn terminal_v0_dispatch(cmd: &str) -> bool {
    match cmd {
        "help" => {
            serial::write_line("apps: terminal commands=help");
            true
        }
        _ => false,
    }
}

fn task_app_terminal_v0() {
    APP_TERMINAL_LAUNCH_OK.store(1, Ordering::Relaxed);

    if terminal_v0_dispatch("help") {
        APP_TERMINAL_HELP_OK.store(1, Ordering::Relaxed);
    }

    APP_TERMINAL_DONE.store(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_app_terminal_v0() {
    APP_TERMINAL_DONE.store(0, Ordering::Relaxed);
    APP_TERMINAL_LAUNCH_OK.store(0, Ordering::Relaxed);
    APP_TERMINAL_HELP_OK.store(0, Ordering::Relaxed);

    let task_id =
        scheduler::spawn_task_with_fn_prio_name(task_app_terminal_v0, 20, "app-terminal-v0");
    let mut done = false;

    if let Some(tid) = task_id {
        let start = scheduler::ticks();
        while scheduler::ticks().saturating_sub(start) < 120 {
            if !scheduler::dispatch_once() {
                idle::sleep_for_ticks(1);
            }
            if scheduler::task_state(tid) == scheduler::TaskState::Empty
                && APP_TERMINAL_DONE.load(Ordering::Relaxed) == 1
            {
                done = true;
                break;
            }
        }
    }

    let launch_pass =
        task_id.is_some() && done && APP_TERMINAL_LAUNCH_OK.load(Ordering::Relaxed) == 1;
    let help_pass = task_id.is_some() && done && APP_TERMINAL_HELP_OK.load(Ordering::Relaxed) == 1;

    serial::write_line(if launch_pass {
        "apps: terminal launch PASS"
    } else {
        "apps: terminal launch FAIL"
    });

    serial::write_line(if help_pass {
        "apps: terminal command help PASS"
    } else {
        "apps: terminal command help FAIL"
    });
}
