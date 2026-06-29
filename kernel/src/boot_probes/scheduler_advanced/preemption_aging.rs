use super::*;

mod aging;
mod preemption;
mod telemetry;
mod toggle;

pub(crate) use aging::probe_priority_aging;
pub(crate) use preemption::probe_preemption;
pub(crate) use telemetry::probe_aging_telemetry;
pub(crate) use toggle::probe_aging_toggle;

static AGING_LOW_RAN: AtomicU64 = AtomicU64::new(0);
static AGING_HIGH_ITERS: AtomicU64 = AtomicU64::new(0);
static AGING_STOP: AtomicU64 = AtomicU64::new(0);

fn task_aging_high_hog() {
    while AGING_LOW_RAN.load(Ordering::Relaxed) == 0 && AGING_STOP.load(Ordering::Relaxed) == 0 {
        AGING_HIGH_ITERS.fetch_add(1, Ordering::Relaxed);
        core::hint::spin_loop();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_aging_low_once() {
    AGING_LOW_RAN.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}
