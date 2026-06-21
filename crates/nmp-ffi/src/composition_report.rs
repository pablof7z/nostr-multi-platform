//! ADR-0049 Part 2 — composition-report FFI accessor.
//!
//! Sole entry point: [`nmp_app_composition_report`]. A host (or a diagnostics
//! screen) calls this to render "which modules / parsers / projections / wiring
//! slots did the composition install, which yielded to an app override, and
//! which were dropped because they were wired after `nmp_app_start`?" — NMP's
//! analog of Spring Boot's `ConditionEvaluationReport`.
//!
//! ## Why a dedicated FFI symbol (not a snapshot-projection)
//!
//! The composition ledger is diagnostic, pulled on demand. Piping it through
//! every snapshot tick would clone the whole record vec on every kernel
//! mutation for data that only changes at host-init. A dedicated pull accessor
//! matches the cost model: zero work until a host asks, then one JSON encode of
//! the append-only ledger (mirrors `nmp_app_recent_routing_decisions`).
//!
//! ## Doctrine
//!
//! - **D0** — the ledger and its record types live in
//!   `nmp-core::kernel::composition_ledger` (consumer-side, no app nouns); this
//!   file is just the C-ABI wrapper.
//! - **D6** — a null `app` or a serialisation failure collapses to a
//!   well-formed empty document (`{"schema_version":1,"count":0,"records":[]}`).
//!   Never returns NULL for a non-null `app`.
//! - **D8** — the read is one `Mutex` lock + clone of the record vec, then one
//!   JSON encode per call; never on the producer path.

use std::ffi::{c_char, CString};

use serde_json::json;

use super::{app_ref, NmpApp};

/// Heap-allocated empty-ledger payload. Returned for a null `app` or a
/// serialisation failure — the host sees a well-formed document either way (D6).
fn empty_payload() -> *mut c_char {
    let v = json!({
        "schema_version": nmp_core::COMPOSITION_REPORT_SCHEMA_VERSION,
        "count": 0,
        "records": [],
    });
    let s = serde_json::to_string(&v)
        .unwrap_or_else(|_| String::from(r#"{"schema_version":1,"count":0,"records":[]}"#));
    CString::new(s)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

/// Return a heap-owned NUL-terminated JSON snapshot of the app's composition
/// ledger. The caller MUST release the returned pointer via
/// [`super::free::nmp_free_string`].
///
/// Payload shape (stable, schema-versioned):
///
/// ```text
/// {
///   "schema_version": 1,
///   "count": 7,
///   "records": [
///     { "seam": "action_registry", "key": "nmp.nip02.follow",
///       "provider": "nmp_nip02::FollowModule", "disposition": "Installed" },
///     { "seam": "action_registry", "key": "nmp.publish",
///       "provider": "app::MyPublish", "disposition": "ReplacedPrevious",
///       "replaced": "nmp_core::publish::PublishModule" },
///     { "seam": "routing_substrate", "key": "routing_substrate",
///       "provider": "routing_substrate", "disposition": "DroppedLateWiring" }
///   ]
/// }
/// ```
///
/// `disposition` is one of `Installed`, `ReplacedPrevious`,
/// `YieldedToExisting`, `DroppedLateWiring`. `replaced` is present only for
/// `ReplacedPrevious` / `YieldedToExisting`.
///
/// D6: returns the empty-ledger payload — never NULL — when `app` is null or
/// when JSON encoding fails.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_composition_report(app: *mut NmpApp) -> *mut c_char {
    let Some(app) = app_ref(app) else {
        return empty_payload();
    };
    let value = app.composition_ledger().to_json();
    match serde_json::to_string(&value) {
        Ok(s) => CString::new(s)
            .unwrap_or_else(|_| c"{}".to_owned())
            .into_raw(),
        Err(_) => empty_payload(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{nmp_app_free, nmp_app_new};
    use std::ffi::CStr;

    fn decode(ptr: *mut c_char) -> serde_json::Value {
        assert!(!ptr.is_null());
        // SAFETY: ptr is from CString::into_raw above.
        let s = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        serde_json::from_str(&s).expect("payload is valid JSON")
    }

    #[test]
    fn null_app_returns_empty_payload_not_null() {
        let ptr = nmp_app_composition_report(std::ptr::null_mut());
        let v = decode(ptr);
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["count"], 0);
        assert_eq!(v["records"].as_array().unwrap().len(), 0);
        crate::free::nmp_free_string(ptr);
    }

    #[test]
    fn fresh_app_payload_is_well_formed() {
        let app = nmp_app_new();
        let ptr = nmp_app_composition_report(app);
        let v = decode(ptr);
        assert_eq!(v["schema_version"], 1);
        assert!(v["records"].is_array());
        assert!(v["count"].is_u64());
        crate::free::nmp_free_string(ptr);
        nmp_app_free(app);
    }

    #[test]
    fn default_action_registration_is_recorded() {
        use nmp_core::substrate::{ActionContext, ActionId, ActionModule, ActionRejection};

        // A trivial default module to register through the host seam.
        struct ProbeModule;
        impl ActionModule for ProbeModule {
            type Action = serde_json::Value;
            const NAMESPACE: &'static str = "test.composition.probe";
            fn start(
        &self,
                _ctx: &mut ActionContext,
                _action: Self::Action,
            ) -> Result<(), ActionRejection> {
                Ok(())
            }
            fn execute(
        &self,
                _action: Self::Action,
                _correlation_id: &str,
                _send: &dyn Fn(nmp_core::ActorCommand),
            ) -> Result<(), String> {
                Ok(())
            }
        }

        let app = nmp_app_new();
        // SAFETY: exclusive borrow of a valid pointer from nmp_app_new; no
        // other reference aliases it in this test.
        let app_mut = unsafe { &mut *app };
        let installed = app_mut.register_default_action(ProbeModule);
        assert!(installed, "first default registration installs");

        let ptr = nmp_app_composition_report(app);
        let v = decode(ptr);
        let records = v["records"].as_array().unwrap();
        let probe = records
            .iter()
            .find(|r| r["key"] == "test.composition.probe")
            .expect("probe registration recorded in the ledger");
        assert_eq!(probe["seam"], "action_registry");
        assert_eq!(probe["disposition"], "Installed");

        crate::free::nmp_free_string(ptr);
        nmp_app_free(app);
    }
}
