//! Shared test helpers for the per-domain FFI test sub-modules.
//!
//! Kept `pub(super)` so sibling test modules can reach them via
//! `super::helpers::{dispatch, run_module_execute, register_app}` without
//! exposing the helpers beyond the test tree.

use std::cell::RefCell;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{ActionContext, ActionModule};
use nmp_ffi::NmpApp;

use super::super::{ChirpHandle, NmpRegisterStatus, nmp_app_chirp_register};

#[cfg(feature = "marmot")]
static MARMOT_DB_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Run an `ActionModule`'s typed executor once and capture **every**
/// `ActorCommand` it sends, in order. Mirrors `nmp_nip17::dm_relay_list`'s
/// test pattern — the canonical post-ADR-0027 executor probe.
///
/// Returns `Ok(vec![])` for an executor that returns `Ok(())` without
/// sending any command (a valid no-op); returns `Err(...)` only when the
/// executor itself returns `Err(...)`. Earlier this helper kept only the
/// last `send()` call in a `RefCell<Option<_>>`, silently dropping
/// multi-command executors (e.g. `EnsureInterest` followed by
/// `RecordActionSuccess`).
pub(super) fn run_module_execute<M: ActionModule + Default>(
    input: M::Action,
) -> Result<Vec<ActorCommand>, String> {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    // ADR-0052 rung 5.2: `execute` takes `&self` (the module value carries its
    // dependencies). The NIP-29 test modules probed here are stateless unit
    // structs, so a `Default`-constructed instance is the canonical handle.
    let ctx = ActionContext::default();
    M::default().execute(&ctx, input, "test-cid", &|cmd| {
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
    assert!(
        !handle.is_null(),
        "register_app: handle is null after Ok status"
    );
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

#[cfg(feature = "marmot")]
pub(super) struct MarmotTestRegistration {
    handle: *mut nmp_marmot::ffi::MarmotHandle,
    db_dir: std::path::PathBuf,
}

#[cfg(feature = "marmot")]
impl Drop for MarmotTestRegistration {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            nmp_marmot::ffi::nmp_marmot_unregister(self.handle);
            self.handle = std::ptr::null_mut();
        }
        let _ = std::fs::remove_dir_all(&self.db_dir);
    }
}

#[cfg(feature = "marmot")]
fn marmot_test_db_dir(scope: &str) -> std::path::PathBuf {
    let seq = MARMOT_DB_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "nmp-chirp-marmot-{scope}-{}-{seq}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create Marmot test temp db dir");
    dir
}

/// Register Marmot through the same Rust helper Chirp's nsec sign-in path uses,
/// so feature-unified tests exercise the production action/projection surface.
#[cfg(feature = "marmot")]
pub(super) fn register_marmot_for_test(app: *mut NmpApp, scope: &str) -> MarmotTestRegistration {
    let db_dir = marmot_test_db_dir(scope);
    let secret =
        std::ffi::CString::new("0101010101010101010101010101010101010101010101010101010101010101")
            .expect("static secret has no NUL");
    let db_dir_c = std::ffi::CString::new(db_dir.to_string_lossy().into_owned())
        .expect("temp db path has no NUL");
    let service_id = std::ffi::CString::new(nmp_chirp_config::CHIRP_MARMOT_KEYRING_SERVICE_ID)
        .expect("static service id has no NUL");

    let handle = nmp_marmot::ffi::register_with_secret_hex(
        app,
        secret.as_ptr(),
        db_dir_c.as_ptr(),
        service_id.as_ptr(),
    );
    assert!(
        !handle.is_null(),
        "Marmot registration must succeed for the full-composition gate"
    );

    MarmotTestRegistration { handle, db_dir }
}
