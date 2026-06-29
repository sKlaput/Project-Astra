use super::*;

// --- priority-mutation probe support ---
// Three tasks at mid priority (128). After all three are enqueued, we bump
// task C to priority 0 (highest urgency). The probe then dequeues one task
// and verifies it is C, proving the mutation won the next dequeue.
static PMUT_ORDER: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static PMUT_SEQ: AtomicU64 = AtomicU64::new(0);

fn task_pmut_record() {
    let pos = PMUT_SEQ.fetch_add(1, Ordering::Relaxed) as usize;
    if pos < 3 {
        // Record which task_id ran at this position.
        if let Some(id) = scheduler::current_task() {
            PMUT_ORDER[pos].store(id.0, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_priority_mutation() {
    PMUT_SEQ.store(0, Ordering::Relaxed);
    PMUT_ORDER[0].store(0, Ordering::Relaxed);
    PMUT_ORDER[1].store(0, Ordering::Relaxed);
    PMUT_ORDER[2].store(0, Ordering::Relaxed);

    // Spawn A, B, C all at mid priority 128 — they enter the ring in FIFO order.
    let id_a = scheduler::spawn_task_with_fn_prio_name(task_pmut_record, 128, "pmut-A").unwrap();
    let _id_b = scheduler::spawn_task_with_fn_prio_name(task_pmut_record, 128, "pmut-B").unwrap();
    let id_c = scheduler::spawn_task_with_fn_prio_name(task_pmut_record, 128, "pmut-C").unwrap();

    // Verify initial priorities are all 128.
    let prio_before_a = scheduler::task_priority(id_a);
    let prio_before_c = scheduler::task_priority(id_c);

    // Bump C to highest urgency — must beat A and B on the next dequeue.
    let bump_ok = scheduler::set_task_priority(id_c, 0);
    let prio_after_c = scheduler::task_priority(id_c);

    // Dispatch once: should pick C (priority 0).
    scheduler::dispatch_once();
    // Drain A and B.
    scheduler::dispatch_once();
    scheduler::dispatch_once();

    let first_ran = PMUT_ORDER[0].load(Ordering::Relaxed);
    let c_ran_first = first_ran == id_c.0;

    serial::write_str("scheduler: priority-mutation prio_before=");
    serial::write_u64(prio_before_a as u64);
    serial::write_str(",");
    serial::write_u64(prio_before_c as u64);
    serial::write_str(" prio_after_c=");
    serial::write_u64(prio_after_c as u64);
    serial::write_str(" bump_ok=");
    serial::write_u64(bump_ok as u64);
    serial::write_str(" c_first=");
    serial::write_u64(c_ran_first as u64);
    serial::write_line("");

    let pass =
        prio_before_a == 128 && prio_before_c == 128 && bump_ok && prio_after_c == 0 && c_ran_first;
    serial::write_line(if pass {
        "scheduler: priority-mutation PASS"
    } else {
        "scheduler: priority-mutation FAIL"
    });
}
