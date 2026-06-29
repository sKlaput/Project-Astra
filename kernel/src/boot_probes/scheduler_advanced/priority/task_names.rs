use super::*;

static NAME_A_MATCH: AtomicU64 = AtomicU64::new(0);
static NAME_B_MATCH: AtomicU64 = AtomicU64::new(0);
static NAME_C_MATCH: AtomicU64 = AtomicU64::new(0);

fn task_name_a() {
    // Verify that the name is visible from inside the task via current_task().
    if let Some(id) = scheduler::current_task() {
        if scheduler::task_name(id) == "alpha" {
            NAME_A_MATCH.store(1, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}
fn task_name_b() {
    if let Some(id) = scheduler::current_task() {
        if scheduler::task_name(id) == "beta" {
            NAME_B_MATCH.store(1, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}
fn task_name_c() {
    if let Some(id) = scheduler::current_task() {
        if scheduler::task_name(id) == "gamma" {
            NAME_C_MATCH.store(1, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_names() {
    NAME_A_MATCH.store(0, Ordering::Relaxed);
    NAME_B_MATCH.store(0, Ordering::Relaxed);
    NAME_C_MATCH.store(0, Ordering::Relaxed);

    // Spawn three named tasks; verify name is retrievable before dispatch.
    let id_a = scheduler::spawn_task_with_fn_prio_name(task_name_a, 128, "alpha").unwrap();
    let id_b = scheduler::spawn_task_with_fn_prio_name(task_name_b, 128, "beta").unwrap();
    let id_c = scheduler::spawn_task_with_fn_prio_name(task_name_c, 128, "gamma").unwrap();

    let pre_a = scheduler::task_name(id_a) == "alpha";
    let pre_b = scheduler::task_name(id_b) == "beta";
    let pre_c = scheduler::task_name(id_c) == "gamma";

    // Drain all three tasks.
    let deadline = scheduler::ticks() + 80;
    while scheduler::ticks() < deadline {
        if !scheduler::dispatch_once() {
            break;
        }
    }
    while scheduler::dispatch_once() {}

    let post_a = NAME_A_MATCH.load(Ordering::Relaxed);
    let post_b = NAME_B_MATCH.load(Ordering::Relaxed);
    let post_c = NAME_C_MATCH.load(Ordering::Relaxed);

    serial::write_str("scheduler: task-names pre=");
    serial::write_u64(pre_a as u64);
    serial::write_u64(pre_b as u64);
    serial::write_u64(pre_c as u64);
    serial::write_str(" in-task=");
    serial::write_u64(post_a);
    serial::write_u64(post_b);
    serial::write_u64(post_c);
    serial::write_line("");

    let pass = pre_a && pre_b && pre_c && post_a == 1 && post_b == 1 && post_c == 1;
    serial::write_line(if pass {
        "scheduler: task-names PASS"
    } else {
        "scheduler: task-names FAIL"
    });
}
