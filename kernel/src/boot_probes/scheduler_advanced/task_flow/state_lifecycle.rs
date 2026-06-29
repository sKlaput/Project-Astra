use super::*;

pub(crate) fn probe_scheduler_task_state() {
    let id = scheduler::spawn_task();

    if let Some(task) = id {
        let state = scheduler::task_state(task);
        serial::write_str("scheduler: task-state task_id=");
        serial::write_u64(task.0);
        serial::write_str(" state=");
        serial::write_str(match state {
            scheduler::TaskState::Ready => "Ready",
            scheduler::TaskState::Running => "Running",
            scheduler::TaskState::Sleeping => "Sleeping",
            scheduler::TaskState::Empty => "Empty",
        });
        serial::write_line("");
        // Drain again to keep the queue empty for the idle loop.
        scheduler::dequeue_next();
    }
}

pub(crate) fn probe_task_lifecycle() {
    // Spawn 3 tasks — ring is empty at this point so all must succeed.
    let ta = scheduler::spawn_task();
    let tb = scheduler::spawn_task();
    let tc = scheduler::spawn_task();

    // Verify all are Ready immediately after spawn.
    let mut ready_count: u64 = 0;
    for t in [ta, tb, tc] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Ready {
                ready_count += 1;
            }
        }
    }

    // Simulate being scheduled: dequeue each from the ring.
    scheduler::dequeue_next();
    scheduler::dequeue_next();
    scheduler::dequeue_next();

    // Simulate task completion: exit clears the metadata table entry.
    for t in [ta, tb, tc] {
        if let Some(task) = t {
            scheduler::exit_task(task);
        }
    }

    // Verify all are now Empty.
    let mut empty_count: u64 = 0;
    for t in [ta, tb, tc] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_count += 1;
            }
        }
    }

    serial::write_str("scheduler: task-lifecycle ready=");
    serial::write_u64(ready_count);
    serial::write_str("/3 empty=");
    serial::write_u64(empty_count);
    serial::write_str("/3");
    serial::write_line("");
}

pub(crate) fn probe_scheduler_ring_overflow() {
    // Fill the ring to capacity, then attempt one extra spawn — it must return None.
    let cap = scheduler::ring_capacity();
    let mut spawned: usize = 0;
    let mut dropped: usize = 0;

    for _ in 0..=cap {
        match scheduler::spawn_task() {
            Some(_) => spawned += 1,
            None => dropped += 1,
        }
    }

    // Drain the ring so the idle-decision logic stays clean.
    let mut drained: usize = 0;
    while scheduler::dequeue_next().is_some() {
        drained += 1;
    }

    serial::write_str("scheduler: ring-overflow spawned=");
    serial::write_u64(spawned as u64);
    serial::write_str(" dropped=");
    serial::write_u64(dropped as u64);
    serial::write_str(" drained=");
    serial::write_u64(drained as u64);
    serial::write_line("");
}
