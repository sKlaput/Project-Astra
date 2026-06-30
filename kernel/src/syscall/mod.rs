pub mod authz;
pub mod handlers;
pub mod memory;

pub use authz::{
    SecurityAuthzSnapshot, AUTHZ_REASON_DENY_DEFAULT, AUTHZ_REASON_DENY_PRIVILEGED_GROUP,
    AUTHZ_REASON_DENY_UNKNOWN_SYSCALL,
};

/// Unknown syscall sentinel.
pub const SYS_ENOSYS: u64 = u64::MAX;

pub const SYS_NOP: u64 = 0;
pub const SYS_ADD: u64 = 1;
pub const SYS_MAX: u64 = 2;
pub const SYS_XORROT: u64 = 3;
pub const SYS_TICKS: u64 = 4;
pub const SYS_TASK_ID: u64 = 5;
pub const SYS_SIGNAL_SET: u64 = 6;
pub const SYS_SIGNAL_PENDING: u64 = 7;
pub const SYS_SIGNAL_CLEAR: u64 = 8;
pub const SYS_SIGNAL_WAIT_UNTIL: u64 = 9;
pub const SYS_SIGNAL_WAIT: u64 = 10;
pub const SYS_SIGNAL_WAIT_ALL_UNTIL: u64 = 11;
pub const SYS_SIGNAL_MASK_GET: u64 = 12;
pub const SYS_SIGNAL_BLOCK: u64 = 13;
pub const SYS_SIGNAL_UNBLOCK: u64 = 14;
pub const SYS_SIGNAL_WAIT_CONSUME_UNTIL: u64 = 15;
pub const SYS_SIGNAL_WAIT_CONSUME: u64 = 16;
pub const SYS_SIGNAL_WAIT_ALL_CONSUME_UNTIL: u64 = 17;
pub const SYS_SIGNAL_WAIT_ALL_CONSUME: u64 = 18;
pub const SYS_WRITE_CONSOLE: u64 = 19;
pub const SYS_YIELD: u64 = 20;
pub const SYS_EXIT: u64 = 21;
pub const SYS_SEND_MSG: u64 = 22;
pub const SYS_RECV_MSG: u64 = 23;
pub const SYS_GET_FB_INFO: u64 = 24;
pub const SYS_DRAW_RECT: u64 = 25;
pub const SYS_DRAW_PIXEL: u64 = 26;
pub const SYS_DRAW_TEXT: u64 = 27;
pub const SYS_MAP_FB: u64 = 28;

/// Dispatch a syscall by number.
///
/// Argument order matches the project ABI baseline: rdi, rsi, rdx, r10, r8, r9.
pub fn dispatch(nr: u64, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> u64 {
    let table_len = handlers::SYSCALL_TABLE.len();
    let (allowed, reason) = authz::authorize_syscall(nr, table_len);
    authz::authz_record(reason, allowed);
    if !allowed {
        return SYS_ENOSYS;
    }

    match handlers::SYSCALL_TABLE.get(nr as usize) {
        Some(handler) => handler(a, b, c, d, e, f),
        None => {
            authz::authz_record(AUTHZ_REASON_DENY_DEFAULT, false);
            SYS_ENOSYS
        }
    }
}

/// Number of syscall slots currently implemented.
pub fn table_len() -> u64 {
    handlers::SYSCALL_TABLE.len() as u64
}

pub fn security_authz_snapshot() -> SecurityAuthzSnapshot {
    authz::security_authz_snapshot()
}

pub fn security_probe_record_user_authz(nr: u64) -> bool {
    authz::security_probe_record_user_authz(nr, handlers::SYSCALL_TABLE.len())
}
