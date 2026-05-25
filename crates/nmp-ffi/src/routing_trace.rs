//! V-51 phase 2 — routing-trace FFI snapshot accessor.
//!
//! Sole entry point: [`nmp_app_recent_routing_decisions`]. The Chirp shell
//! (iOS) calls this to render the "why did event Y go to relay B?"
//! inspector — the user-facing payoff phase 3 will paint over the JSON
//! payload this symbol returns.
//!
//! ## Why a dedicated FFI symbol (not a snapshot-projection)
//!
//! The host-extensible snapshot-projection seam
//! (`nmp_app_register_snapshot_projection`) is the right home for app
//! nouns — wallet status, marketplace listings, todo items, etc. — that
//! a host wants delivered on every snapshot tick. The routing trace is
//! diagnostic, pulled on demand from a long-press / settings toggle (V-51
//! phase 3); piping it through every snapshot tick would needlessly clone
//! ≤ 64 publish + 64 subscription entries on every kernel mutation.
//!
//! A dedicated pull accessor matches the cost model: zero work until a
//! host asks, then one JSON encode of the current rings.
//!
//! ## Doctrine
//!
//! - **D0** — the DTO is built in `nmp-core::kernel::routing_trace_dto`
//!   (consumer-side, no app nouns); this file is just the C ABI wrapper.
//! - **D6** — a null `app`, a kernel that hasn't constructed its
//!   projection yet, or a serialisation failure all collapse to a
//!   well-formed `{"schema_version":1,"capacity":0,"publishes":[],
//!   "subscriptions":[]}` document. Never returns NULL for a non-null
//!   `app` so the host's decoder never branches on null-vs-empty.
//! - **D8** — the read is a `RwLock::read().iter().cloned().collect()`
//!   per ring (the projection's own snapshot accessors); the JSON encode
//!   runs once per call, never on the producer path.

use std::ffi::{c_char, CString};

use nmp_core::projection_to_json;
use serde_json::json;

use super::{app_ref, NmpApp};

/// Heap-allocated empty-rings payload. Returned whenever the kernel
/// projection is not yet bound (pre-`nmp_app_start`) or a serialisation
/// failure prevents rendering the real payload — the host sees a
/// well-formed document either way (D6).
fn empty_payload() -> *mut c_char {
    let v = json!({
        "schema_version": nmp_core::ROUTING_TRACE_SCHEMA_VERSION,
        "capacity": 0,
        "publishes": [],
        "subscriptions": [],
    });
    // Serialise the empty document; if even that fails, fall back to a
    // const C string. Both paths are total — the host never sees NULL.
    let s = serde_json::to_string(&v).unwrap_or_else(|_| {
        String::from(r#"{"schema_version":1,"capacity":0,"publishes":[],"subscriptions":[]}"#)
    });
    CString::new(s)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

/// Return a heap-owned NUL-terminated JSON snapshot of the kernel's
/// recent routing decisions. The caller MUST release the returned pointer
/// via [`super::capability::nmp_app_free_string`].
///
/// Payload shape (stable, schema-versioned):
///
/// ```text
/// {
///   "schema_version": 1,
///   "capacity": 64,
///   "publishes":     [ { at_ms, kind, author, event_id_short,
///                        explicit_targets_set, urls: [ {url, lanes: [...]} ] } ],
///   "subscriptions": [ { at_ms, interest_id, kinds, authors_count,
///                        explicit_targets_set, urls: [...] } ]
/// }
/// ```
///
/// Each `lanes[]` entry is a `{ "kind": "Nip65", "direction": "Write" }`-style
/// object whose discriminator matches the chirp-repl pretty-printer's
/// grammar (`Nip65/Write`, `ClassRouted/<class>/<via>`, `AppRelay/<mode>`,
/// etc.). See `crates/nmp-core/src/kernel/routing_trace_dto.rs` for the
/// canonical shape definition.
///
/// D6: returns the empty-rings payload — never NULL — when `app` is
/// null, when the kernel hasn't yet built its `RoutingTraceProjection`
/// (the actor publishes it into the slot immediately after kernel
/// construction; pre-`nmp_app_start` callers will see this), or when
/// the slot's `Mutex` is poisoned.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_recent_routing_decisions(app: *mut NmpApp) -> *mut c_char {
    let Some(app) = app_ref(app) else {
        return empty_payload();
    };
    let Some(projection) = app.routing_trace() else {
        return empty_payload();
    };
    let value = projection_to_json(&projection);
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

    /// A fresh `NmpApp` (pre-start, no kernel built yet) still returns a
    /// well-formed empty-rings document — never NULL.
    #[test]
    fn null_app_returns_empty_payload_not_null() {
        let ptr = nmp_app_recent_routing_decisions(std::ptr::null_mut());
        assert!(!ptr.is_null());
        // SAFETY: ptr is from CString::into_raw above.
        let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["capacity"], 0);
        assert_eq!(v["publishes"].as_array().unwrap().len(), 0);
        assert_eq!(v["subscriptions"].as_array().unwrap().len(), 0);
        // Free through the public string-free entry point.
        crate::capability::nmp_app_free_string(ptr);
    }

    #[test]
    fn fresh_app_pre_start_returns_empty_payload() {
        let app = nmp_app_new();
        let ptr = nmp_app_recent_routing_decisions(app);
        assert!(!ptr.is_null());
        // SAFETY: ptr is from CString::into_raw above.
        let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        // Pre-start: the actor has not yet published the projection clone
        // into the slot — we return the empty-rings payload (D6).
        assert_eq!(v["schema_version"], 1);
        assert!(v["publishes"].is_array());
        assert!(v["subscriptions"].is_array());
        crate::capability::nmp_app_free_string(ptr);
        nmp_app_free(app);
    }

    #[test]
    fn payload_is_round_trippable_through_serde() {
        let app = nmp_app_new();
        let ptr = nmp_app_recent_routing_decisions(app);
        // SAFETY: ptr is from CString::into_raw above.
        let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
        // Decoding must succeed and the schema_version key must be present —
        // the host's strict Swift Decodable would fail otherwise.
        let v: serde_json::Value = serde_json::from_str(&s).expect("payload is valid JSON");
        assert!(v.is_object());
        assert!(v.get("schema_version").is_some());
        assert!(v.get("capacity").is_some());
        assert!(v.get("publishes").is_some());
        assert!(v.get("subscriptions").is_some());
        crate::capability::nmp_app_free_string(ptr);
        nmp_app_free(app);
    }
}
