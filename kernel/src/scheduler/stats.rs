use core::sync::atomic::{AtomicU64, Ordering};

static STAT_DISPATCH_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_SLEEP_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_WAKE_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_EXIT_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_REQUEUE_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_PREEMPT_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_AGING_BOOSTS: AtomicU64 = AtomicU64::new(0);
static STAT_MAX_WAIT_TICKS: AtomicU64 = AtomicU64::new(0);
static STAT_PARK_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_UNPARK_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_UNPARK_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_SIGNAL_SET_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_SIGNAL_WAKE_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_SIGNAL_WAKE_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
pub struct SchedulerStats {
    pub dispatches: u64,
    pub sleeps: u64,
    pub wakes: u64,
    pub exits: u64,
    pub requeues: u64,
    pub preempts: u64,
    pub aging_boosts: u64,
    pub max_wait_ticks: u64,
    pub parks: u64,
    pub unparks: u64,
    pub unpark_fails: u64,
}

// ---- internal record helpers ------------------------------------------------

pub fn record_dispatch() {
    STAT_DISPATCH_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_sleep() {
    STAT_SLEEP_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_wake() {
    STAT_WAKE_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_exit() {
    STAT_EXIT_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_requeue() {
    STAT_REQUEUE_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_preempt() {
    STAT_PREEMPT_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_aging_boost(wait_ticks: u64) {
    STAT_AGING_BOOSTS.fetch_add(1, Ordering::Relaxed);
    STAT_MAX_WAIT_TICKS.fetch_max(wait_ticks, Ordering::Relaxed);
}
pub fn record_park() {
    STAT_PARK_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_unpark() {
    STAT_UNPARK_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_unpark_fail() {
    STAT_UNPARK_FAIL_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_signal_set() {
    STAT_SIGNAL_SET_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_signal_wake() {
    STAT_SIGNAL_WAKE_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_signal_wake_fail() {
    STAT_SIGNAL_WAKE_FAIL_COUNT.fetch_add(1, Ordering::Relaxed);
}

// ---- public snapshot & individual getters -----------------------------------

pub fn debug_stats_snapshot() -> SchedulerStats {
    SchedulerStats {
        dispatches: STAT_DISPATCH_COUNT.load(Ordering::Relaxed),
        sleeps: STAT_SLEEP_COUNT.load(Ordering::Relaxed),
        wakes: STAT_WAKE_COUNT.load(Ordering::Relaxed),
        exits: STAT_EXIT_COUNT.load(Ordering::Relaxed),
        requeues: STAT_REQUEUE_COUNT.load(Ordering::Relaxed),
        preempts: STAT_PREEMPT_COUNT.load(Ordering::Relaxed),
        aging_boosts: STAT_AGING_BOOSTS.load(Ordering::Relaxed),
        max_wait_ticks: STAT_MAX_WAIT_TICKS.load(Ordering::Relaxed),
        parks: STAT_PARK_COUNT.load(Ordering::Relaxed),
        unparks: STAT_UNPARK_COUNT.load(Ordering::Relaxed),
        unpark_fails: STAT_UNPARK_FAIL_COUNT.load(Ordering::Relaxed),
    }
}

pub fn stat_preempt_count() -> u64 {
    STAT_PREEMPT_COUNT.load(Ordering::Relaxed)
}
pub fn stat_aging_boosts() -> u64 {
    STAT_AGING_BOOSTS.load(Ordering::Relaxed)
}
pub fn stat_max_wait_ticks() -> u64 {
    STAT_MAX_WAIT_TICKS.load(Ordering::Relaxed)
}
pub fn stat_park_count() -> u64 {
    STAT_PARK_COUNT.load(Ordering::Relaxed)
}
pub fn stat_unpark_count() -> u64 {
    STAT_UNPARK_COUNT.load(Ordering::Relaxed)
}
pub fn stat_unpark_fail_count() -> u64 {
    STAT_UNPARK_FAIL_COUNT.load(Ordering::Relaxed)
}
pub fn stat_signal_set_count() -> u64 {
    STAT_SIGNAL_SET_COUNT.load(Ordering::Relaxed)
}
pub fn stat_signal_wake_count() -> u64 {
    STAT_SIGNAL_WAKE_COUNT.load(Ordering::Relaxed)
}
pub fn stat_signal_wake_fail_count() -> u64 {
    STAT_SIGNAL_WAKE_FAIL_COUNT.load(Ordering::Relaxed)
}
