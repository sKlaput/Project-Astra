use core::sync::atomic::{AtomicU64, Ordering};

use crate::{process, scheduler, serial};

static PROCESS_MODEL_WORKER_RAN: AtomicU64 = AtomicU64::new(0);

fn task_process_model_worker() {
    PROCESS_MODEL_WORKER_RAN.store(1, Ordering::Relaxed);
    if let Some(id) = scheduler::current_task() {
        scheduler::exit_task(id);
    }
}

pub(crate) fn probe_process_model() {
    let abi = process::startup_abi_version();
    PROCESS_MODEL_WORKER_RAN.store(0, Ordering::Relaxed);

    let pid = process::spawn_kernel_process("proc-hello", task_process_model_worker, 22);

    let mut seen_running = false;
    let mut task_link_ok = false;
    let mut name_ok = false;
    let mut abi_ok = false;
    let mut uptime_ok = false;

    if let Some(pid) = pid {
        if let Some(task) = process::main_task(pid) {
            task_link_ok = task.0 != 0;
        }
        name_ok = process::process_name_len(pid) == Some("proc-hello".len() as u64);
        abi_ok = process::startup_version(pid) == Some(abi);
        seen_running = process::state(pid) == Some(process::ProcessState::Running);
        // Run one dispatch cycle so the kernel-backed process task executes once.
        let _ = scheduler::dispatch_once();
        uptime_ok = process::uptime_ticks(pid).unwrap_or(0) > 0;
    }

    serial::write_str("process: abi=");
    serial::write_u64(abi as u64);
    serial::write_str(" spawn=");
    serial::write_u64(pid.is_some() as u64);
    serial::write_str(" link=");
    serial::write_u64(task_link_ok as u64);
    serial::write_str(" name=");
    serial::write_u64(name_ok as u64);
    serial::write_str(" ver=");
    serial::write_u64(abi_ok as u64);
    serial::write_str(" run=");
    serial::write_u64(seen_running as u64);
    serial::write_str(" up=");
    serial::write_u64(uptime_ok as u64);
    serial::write_line("");

    let worker_ok = PROCESS_MODEL_WORKER_RAN.load(Ordering::Relaxed) == 1;

    let pass =
        abi == 1 && pid.is_some() && task_link_ok && name_ok && abi_ok && seen_running && worker_ok;

    serial::write_line(if pass {
        "process: model PASS"
    } else {
        "process: model FAIL"
    });
}
