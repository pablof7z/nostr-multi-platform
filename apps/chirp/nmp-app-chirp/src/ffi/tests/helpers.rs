//! Shared test helpers for the per-domain FFI test sub-modules.
//!
//! Kept `pub(super)` so sibling test modules can reach them via
//! `super::helpers::{dispatch, run_module_execute, register_app}` without
//! exposing the helpers beyond the test tree.

use std::cell::RefCell;
use std::ffi::{CStr, CString};

use nmp_core::substrate::ActionModule;
use nmp_core::ActorCommand;
use nmp_ffi::{nmp_app_dispatch_action, nmp_free_string, NmpApp};

use super::super::{nmp_app_chirp_register, ChirpHandle, NmpRegisterStatus};

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

/// Call `nmp_app_chirp_register` with a null viewer_pubkey (the "no viewer"
/// case used by most tests that are testing things other than pubkey
/// validation). Panics if registration fails — that would indicate an
/// unrelated infrastructure problem in the test environment.
pub(super) fn register_app(app: *mut NmpApp) -> *mut ChirpHandle {
    let mut handle: *mut ChirpHandle = std::ptr::null_mut();
    // SAFETY: `app` is a valid pointer from `nmp_app_new`; null viewer_pubkey
    // is explicitly permitted ("no viewer set").
    let status = nmp_app_chirp_register(app, std::ptr::null(), &mut handle);
    assert_eq!(
        status,
        NmpRegisterStatus::Ok as u32,
        "register_app: nmp_app_chirp_register failed with status={status}"
    );
    assert!(!handle.is_null(), "register_app: handle is null after Ok status");
    handle
}

/// Drive `nmp_app_dispatch_action` for `namespace`/`action_json` and
/// return the parsed JSON result. The returned C string is freed.
pub(super) fn dispatch(app: *mut NmpApp, namespace: &str, action_json: &str) -> serde_json::Value {
    let ns = CString::new(namespace).unwrap();
    let body = CString::new(action_json).unwrap();
    let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
    assert!(!ptr.is_null(), "dispatch_action must never return null");
    // SAFETY: `ptr` is a valid C string from `nmp_app_dispatch_action`.
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}
