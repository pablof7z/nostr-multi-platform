//! Shared test helpers for the per-domain FFI test sub-modules.
//!
//! Kept `pub(super)` so sibling test modules can reach them via
//! `super::helpers::{dispatch, run_module_execute, register_app}` without
//! exposing the helpers beyond the test tree.

use std::cell::RefCell;

use nmp_core::substrate::ActionModule;
use nmp_core::ActorCommand;
use nmp_ffi::NmpApp;

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
pub(super) fn run_module_execute<M: ActionModule + Default>(
    input: M::Action,
) -> Result<Vec<ActorCommand>, String> {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    // ADR-0052 rung 5.2: `execute` takes `&self` (the module value carries its
    // dependencies). The NIP-29 test modules probed here are stateless unit
    // structs, so a `Default`-constructed instance is the canonical handle.
    M::default().execute(input, "test-cid", &|cmd| {
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

/// Drive the typed **byte** doorway for `namespace`/`action_json` and return the
/// parsed JSON result envelope.
///
/// ADR-0064 / Cut-B (#1756): the Chirp crates dispatch exclusively through the
/// byte doorway now, so this probe goes through the SAME
/// `crate::dispatch_bytes` seam the production callers use — `action_json` (the
/// canonical action body) is encoded to the namespace's typed `ActionPayload`,
/// enveloped with a host-minted correlation id, and handed to
/// `nmp_app_dispatch_action_bytes`. The returned `{"correlation_id"}` /
/// `{"error"}` envelope is parsed into a `serde_json::Value` so the existing
/// per-domain assertions are unchanged in shape.
pub(super) fn dispatch(app: *mut NmpApp, namespace: &str, action_json: &str) -> serde_json::Value {
    match crate::dispatch_bytes::dispatch_action_bytes_for(app, namespace, action_json) {
        Ok(correlation_id) => serde_json::json!({ "correlation_id": correlation_id }),
        Err(error) => serde_json::json!({ "error": error }),
    }
}
