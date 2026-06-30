use core::sync::atomic::{AtomicU64, Ordering};

use super::{
    SYS_SIGNAL_BLOCK, SYS_SIGNAL_CLEAR, SYS_SIGNAL_MASK_GET, SYS_SIGNAL_PENDING, SYS_SIGNAL_SET,
    SYS_SIGNAL_UNBLOCK, SYS_SIGNAL_WAIT, SYS_SIGNAL_WAIT_ALL_CONSUME,
    SYS_SIGNAL_WAIT_ALL_CONSUME_UNTIL, SYS_SIGNAL_WAIT_ALL_UNTIL, SYS_SIGNAL_WAIT_CONSUME,
    SYS_SIGNAL_WAIT_CONSUME_UNTIL, SYS_SIGNAL_WAIT_UNTIL,
};

pub const AUTHZ_REASON_ALLOW: u64 = 0;
pub const AUTHZ_REASON_DENY_UNKNOWN_SYSCALL: u64 = 1;
pub const AUTHZ_REASON_DENY_DEFAULT: u64 = 2;
pub const AUTHZ_REASON_DENY_PRIVILEGED_GROUP: u64 = 3;

#[derive(Clone, Copy, Debug)]
pub struct SecurityAuthzSnapshot {
    pub checks: u64,
    pub denied: u64,
    pub last_reason: u64,
    pub deny_unknown: u64,
    pub deny_default: u64,
    pub deny_privileged: u64,
}

static AUTHZ_CHECKS: AtomicU64 = AtomicU64::new(0);
static AUTHZ_DENIED: AtomicU64 = AtomicU64::new(0);
static AUTHZ_LAST_REASON: AtomicU64 = AtomicU64::new(AUTHZ_REASON_ALLOW);
static AUTHZ_DENY_UNKNOWN: AtomicU64 = AtomicU64::new(0);
static AUTHZ_DENY_DEFAULT: AtomicU64 = AtomicU64::new(0);
static AUTHZ_DENY_PRIVILEGED: AtomicU64 = AtomicU64::new(0);

pub fn authz_record(reason: u64, allowed: bool) {
    AUTHZ_CHECKS.fetch_add(1, Ordering::Relaxed);
    if !allowed {
        AUTHZ_DENIED.fetch_add(1, Ordering::Relaxed);
        match reason {
            AUTHZ_REASON_DENY_UNKNOWN_SYSCALL => {
                AUTHZ_DENY_UNKNOWN.fetch_add(1, Ordering::Relaxed);
            }
            AUTHZ_REASON_DENY_DEFAULT => {
                AUTHZ_DENY_DEFAULT.fetch_add(1, Ordering::Relaxed);
            }
            AUTHZ_REASON_DENY_PRIVILEGED_GROUP => {
                AUTHZ_DENY_PRIVILEGED.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
    AUTHZ_LAST_REASON.store(reason, Ordering::Relaxed);
}

fn is_privileged_syscall_group(nr: u64) -> bool {
    matches!(
        nr,
        SYS_SIGNAL_SET
            | SYS_SIGNAL_PENDING
            | SYS_SIGNAL_CLEAR
            | SYS_SIGNAL_WAIT_UNTIL
            | SYS_SIGNAL_WAIT
            | SYS_SIGNAL_WAIT_ALL_UNTIL
            | SYS_SIGNAL_MASK_GET
            | SYS_SIGNAL_BLOCK
            | SYS_SIGNAL_UNBLOCK
            | SYS_SIGNAL_WAIT_CONSUME_UNTIL
            | SYS_SIGNAL_WAIT_CONSUME
            | SYS_SIGNAL_WAIT_ALL_CONSUME_UNTIL
            | SYS_SIGNAL_WAIT_ALL_CONSUME
    )
}

pub fn authorize_syscall_for_caller(nr: u64, caller_is_user: bool, table_len: usize) -> (bool, u64) {
    if nr >= table_len as u64 {
        return (false, AUTHZ_REASON_DENY_UNKNOWN_SYSCALL);
    }

    if caller_is_user && is_privileged_syscall_group(nr) {
        return (false, AUTHZ_REASON_DENY_PRIVILEGED_GROUP);
    }

    (true, AUTHZ_REASON_ALLOW)
}

pub fn authorize_syscall(nr: u64, table_len: usize) -> (bool, u64) {
    let caller_is_user = crate::scheduler::current_task()
        .map(crate::scheduler::is_user_task)
        .unwrap_or(false);
    authorize_syscall_for_caller(nr, caller_is_user, table_len)
}

pub fn security_authz_snapshot() -> SecurityAuthzSnapshot {
    SecurityAuthzSnapshot {
        checks: AUTHZ_CHECKS.load(Ordering::Relaxed),
        denied: AUTHZ_DENIED.load(Ordering::Relaxed),
        last_reason: AUTHZ_LAST_REASON.load(Ordering::Relaxed),
        deny_unknown: AUTHZ_DENY_UNKNOWN.load(Ordering::Relaxed),
        deny_default: AUTHZ_DENY_DEFAULT.load(Ordering::Relaxed),
        deny_privileged: AUTHZ_DENY_PRIVILEGED.load(Ordering::Relaxed),
    }
}

pub fn security_probe_record_user_authz(nr: u64, table_len: usize) -> bool {
    let (allowed, reason) = authorize_syscall_for_caller(nr, true, table_len);
    authz_record(reason, allowed);
    allowed
}
