//! Test-support synchronization FFI.
//!
//! Gated with `test-support`; not part of the production ABI.
#![cfg(any(test, feature = "test-support"))]

use super::{NmpApp, app_ref};
use std::time::Duration;

/// Block until the actor dispatches every command enqueued before this call.
///
/// Returns `true` when the barrier ack arrives before `timeout_ms`, otherwise
/// `false`. Harnesses use this as the deterministic replacement for blind
/// sleeps after fire-and-forget FFI commands.
#[no_mangle]
pub extern "C" fn nmp_app_wait_barrier(app: *mut NmpApp, timeout_ms: u64) -> bool {
    let Some(app) = app_ref(app) else {
        return false;
    };
    nmp_core::testing::wait_barrier(&app.actor_sender(), Duration::from_millis(timeout_ms))
}
