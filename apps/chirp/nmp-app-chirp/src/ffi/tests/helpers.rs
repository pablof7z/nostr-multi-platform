//! Shared test helpers for the per-domain FFI test sub-modules.
//!
//! Kept `pub(super)` so sibling test modules can reach them via
//! `super::helpers::{dispatch, run_module_execute}` without exposing the
//! helpers beyond the test tree.

use std::cell::RefCell;
use std::ffi::{CStr, CString};

use nmp_core::substrate::ActionModule;
use nmp_core::{nmp_app_dispatch_action, nmp_app_free_string, ActorCommand, NmpApp};

/// Run an `ActionModule`'s typed executor once and capture **every**
/// `ActorCommand` it sends, in order. Mirrors `nmp_nip17::dm_relay_list`'s
/// test pattern — the canonical post-ADR-0027 executor probe.
///
/// Returns `Ok(vec![])` for an executor that returns `Ok(())` without
/// sending any command (a valid no-op); returns `Err(...)` only when the
/// executor itself returns `Err(...)`. Earlier this helper kept only the
/// last `send()` call in a `RefCell<Option<_>>`, silently dropping
/// multi-command executors (e.g. `PushInterest` followed by
/// `RecordActionSuccess`).
pub(super) fn run_module_execute<M: ActionModule>(
    input: M::Action,
) -> Result<Vec<ActorCommand>, String> {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    M::execute(input, "test-cid", &|cmd| {
        captured.borrow_mut().push(cmd);
    })?;
    Ok(captured.into_inner())
}

/// Drive `nmp_app_dispatch_action` for `namespace`/`action_json` and
/// return the parsed JSON result. The returned C string is freed.
pub(super) fn dispatch(
    app: *mut NmpApp,
    namespace: &str,
    action_json: &str,
) -> serde_json::Value {
    let ns = CString::new(namespace).unwrap();
    let body = CString::new(action_json).unwrap();
    let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
    assert!(!ptr.is_null(), "dispatch_action must never return null");
    // SAFETY: `ptr` is a valid C string from `nmp_app_dispatch_action`.
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_app_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}
