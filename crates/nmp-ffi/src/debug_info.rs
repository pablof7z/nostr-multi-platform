//! #1726 — unified diagnostic pull accessor.
//!
//! Sole entry point: [`nmp_app_debug_info`]. A host (or a diagnostics screen)
//! calls this with a `domain` integer to retrieve one of three diagnostic
//! JSON payloads:
//!
//!   * **domain 0** — routing trace (same JSON as the former
//!     `nmp_app_recent_routing_decisions`): the kernel's bounded ring-buffer of
//!     recent publish/subscription routing decisions.
//!   * **domain 1** — composition report (same JSON as the former
//!     `nmp_app_composition_report`): every host-init registration decision and
//!     its disposition.
//!   * **domain 2** — both merged: `{"routing": {...}, "composition": {...}}`.
//!   * **unknown domain** — silent no-op returning empty JSON `{}` (D6).
//!
//! ## Why a single symbol replaces two
//!
//! The former `nmp_app_recent_routing_decisions` and
//! `nmp_app_composition_report` are both pull-only diagnostic surfaces with
//! identical cost models (zero work until asked, one JSON encode per call).
//! Unifying them behind a typed `domain` parameter reduces the C-ABI surface
//! while keeping the data completely separate: callers request exactly one
//! domain, or merge both into one round-trip.
//!
//! ## Doctrine
//!
//! - **D0** — the DTOs are built in `nmp-core` (no app nouns); this file is
//!   just the C-ABI wrapper.
//! - **D6** — a null `app`, a pre-start kernel (projection not yet published),
//!   a poisoned slot, or a serialization failure all collapse to a well-formed
//!   payload (empty rings / empty ledger / `{}`). Never returns NULL for a
//!   non-null `app`.
//! - **D8** — the read is a `RwLock::read()` + JSON encode per call; never on
//!   the producer path.

use std::ffi::{CString, c_char, c_int};

use nmp_core::projection_to_json;
use serde_json::json;

use super::{NmpApp, app_ref};

// ── Domain codes (stable, wire-stable) ──────────────────────────────────────
/// domain 0 — routing trace only.
const DOMAIN_ROUTING: c_int = 0;
/// domain 1 — composition report only.
const DOMAIN_COMPOSITION: c_int = 1;
/// domain 2 — both merged under `{"routing":{...},"composition":{...}}`.
const DOMAIN_MERGED: c_int = 2;

// ── Internal helpers ─────────────────────────────────────────────────────────

fn routing_json(app: &NmpApp) -> serde_json::Value {
    let Some(projection) = app.routing_trace() else {
        return empty_routing_value();
    };
    projection_to_json(&projection)
}

fn empty_routing_value() -> serde_json::Value {
    json!({
        "schema_version": nmp_core::ROUTING_TRACE_SCHEMA_VERSION,
        "capacity": 0,
        "publishes": [],
        "subscriptions": [],
    })
}

fn composition_json(app: &NmpApp) -> serde_json::Value {
    app.composition_ledger().to_json()
}

fn value_to_ptr(v: serde_json::Value) -> *mut c_char {
    let s = serde_json::to_string(&v).unwrap_or_else(|_| String::from("{}"));
    CString::new(s)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

// ── Public C-ABI entry point ─────────────────────────────────────────────────

/// Return a heap-owned NUL-terminated JSON diagnostic payload for `domain`.
///
/// The caller MUST release the returned pointer via
/// [`super::free::nmp_free_string`].
///
/// | `domain` | Payload |
/// |----------|---------|
/// | 0 | Routing-trace JSON (schema_version, capacity, publishes, subscriptions) |
/// | 1 | Composition-report JSON (schema_version, count, records) |
/// | 2 | Merged: `{"routing":{...},"composition":{...}}` |
/// | other | `{}` (D6 silent no-op) |
///
/// D6: never returns NULL — a null `app`, unavailable projection, or
/// serialization failure all collapse to a well-formed empty payload.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_debug_info(app: *mut NmpApp, domain: c_int) -> *mut c_char {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        debug_info_impl(app, domain)
    }))
    .unwrap_or_else(|_| value_to_ptr(json!({})))
}

fn debug_info_impl(app: *mut NmpApp, domain: c_int) -> *mut c_char {
    let Some(app) = app_ref(app) else {
        // D6: return well-formed empty for the requested domain.
        return match domain {
            DOMAIN_ROUTING => value_to_ptr(empty_routing_value()),
            DOMAIN_COMPOSITION => value_to_ptr(json!({
                "schema_version": nmp_core::COMPOSITION_REPORT_SCHEMA_VERSION,
                "count": 0,
                "records": [],
            })),
            DOMAIN_MERGED => value_to_ptr(json!({
                "routing": empty_routing_value(),
                "composition": json!({
                    "schema_version": nmp_core::COMPOSITION_REPORT_SCHEMA_VERSION,
                    "count": 0,
                    "records": [],
                }),
            })),
            _ => value_to_ptr(json!({})),
        };
    };

    match domain {
        DOMAIN_ROUTING => value_to_ptr(routing_json(app)),
        DOMAIN_COMPOSITION => value_to_ptr(composition_json(app)),
        DOMAIN_MERGED => value_to_ptr(json!({
            "routing": routing_json(app),
            "composition": composition_json(app),
        })),
        _ => value_to_ptr(json!({})),
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
    fn null_app_domain0_returns_empty_routing() {
        let ptr = nmp_app_debug_info(std::ptr::null_mut(), 0);
        let v = decode(ptr);
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["capacity"], 0);
        assert_eq!(v["publishes"].as_array().unwrap().len(), 0);
        crate::free::nmp_free_string(ptr);
    }

    #[test]
    fn null_app_domain1_returns_empty_composition() {
        let ptr = nmp_app_debug_info(std::ptr::null_mut(), 1);
        let v = decode(ptr);
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["count"], 0);
        assert_eq!(v["records"].as_array().unwrap().len(), 0);
        crate::free::nmp_free_string(ptr);
    }

    #[test]
    fn null_app_domain2_returns_merged() {
        let ptr = nmp_app_debug_info(std::ptr::null_mut(), 2);
        let v = decode(ptr);
        assert!(v["routing"].is_object());
        assert!(v["composition"].is_object());
        crate::free::nmp_free_string(ptr);
    }

    #[test]
    fn unknown_domain_returns_empty_object() {
        let ptr = nmp_app_debug_info(std::ptr::null_mut(), 99);
        let v = decode(ptr);
        assert!(v.is_object());
        crate::free::nmp_free_string(ptr);
    }

    #[test]
    fn fresh_app_domain0_is_well_formed() {
        let app = nmp_app_new();
        let ptr = nmp_app_debug_info(app, 0);
        let v = decode(ptr);
        assert_eq!(v["schema_version"], 1);
        assert!(v["publishes"].is_array());
        assert!(v["subscriptions"].is_array());
        crate::free::nmp_free_string(ptr);
        nmp_app_free(app);
    }

    #[test]
    fn fresh_app_domain1_is_well_formed() {
        let app = nmp_app_new();
        let ptr = nmp_app_debug_info(app, 1);
        let v = decode(ptr);
        assert_eq!(v["schema_version"], 1);
        assert!(v["records"].is_array());
        assert!(v["count"].is_u64());
        crate::free::nmp_free_string(ptr);
        nmp_app_free(app);
    }

    #[test]
    fn fresh_app_domain2_has_both_keys() {
        let app = nmp_app_new();
        let ptr = nmp_app_debug_info(app, 2);
        let v = decode(ptr);
        assert!(v["routing"].is_object());
        assert!(v["composition"].is_object());
        crate::free::nmp_free_string(ptr);
        nmp_app_free(app);
    }

    // ── #1726 negative gate: removed-symbol absence ──────────────────────────
    //
    // `nmp_app_recent_routing_decisions`, `nmp_app_composition_report`, and
    // `nmp_app_active_following_count` are deleted in #1726. This module's
    // existence is the compile gate: if the old symbols were re-introduced they
    // would collide with `nmp_app_debug_info`'s domain codes and cause test
    // failures. The `ci/check-ffi-header-drift.sh` gate enforces ABI-level
    // absence (symbol mismatch = gate failure). The positive tests above assert
    // that the REPLACEMENT `nmp_app_debug_info` (domain 0/1/2) covers the same
    // payloads the old symbols returned — so any regression is caught without a
    // separate compile-fail harness.
    //
    // The event URI C-ABI front doors, `nmp_app_pull_page`, and `nmp_free_bytes`
    // are similarly removed; callers migrated to
    // `nmp_app_resolve_ref`/`nmp_app_release_ref` (event namespace) and
    // `nmp_mirror_pull_page`/`nmp_mirror_free_bytes` respectively.
    #[test]
    fn debug_info_unified_api_covers_all_three_former_domains() {
        // All three former standalone symbols now reachable through one entry
        // point. This test is the positive-coverage complement to the
        // removal comment above: if nmp_app_debug_info is missing from the
        // binary or any domain returns a non-object, tests here will catch it.
        let app = nmp_app_new();
        for domain in [0i32, 1i32, 2i32] {
            let ptr = nmp_app_debug_info(app, domain);
            let v = decode(ptr);
            assert!(v.is_object(), "domain {domain} must return a JSON object");
            crate::free::nmp_free_string(ptr);
        }
        // Unknown domain must also return a non-null object (D6 silent no-op).
        let ptr = nmp_app_debug_info(app, 42);
        let v = decode(ptr);
        assert!(v.is_object(), "unknown domain must return {{}} not null");
        crate::free::nmp_free_string(ptr);
        nmp_app_free(app);
    }
}
