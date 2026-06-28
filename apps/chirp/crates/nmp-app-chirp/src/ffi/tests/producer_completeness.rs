//! Producer-completeness gate for the `payload:Value` → `typed_projections`
//! migration (ADR-0037).
//!
//! Every Chirp host accessor reads its projection **typed-first with a generic
//! JSON fallback** — `typed<K> ?? snapshot?.<k>`. That fallback is a safety net
//! that *masks* any generic projection key whose typed sidecar is missing: the
//! UI still renders (via JSON), and the host's test suite still passes —
//! **through the very net the migration wants to remove.** So "host tests are
//! green" is NOT a valid gate for deleting those fallbacks.
//!
//! This is the gate that *is* valid, asserted on the producer side where the
//! fallback can't hide anything: **every generic `Value` projection key the
//! kernel emits has a typed-sidecar counterpart under the same key.** If the
//! set-difference is empty, the JSON projections subtree is fully redundant and
//! removing the host's `?? snapshot?.<k>` fallbacks cannot lose data. If it is
//! non-empty, each listed key is exactly where fallback-removal would break —
//! type it before deleting `payload:Value`.
//!
//! The generic + typed closures are registered as a PAIR at each call site
//! (e.g. `register.rs` zaps 115/118, follow_list 363/369) reading the same
//! projection slot, so they emit under identical conditions — which is why a
//! keyset-containment check is population-independent: there is never a tick on
//! which the generic key is present and its typed twin is not.
//!
//! Scope: this gate covers the generic `projections` *map* namespace. The
//! top-level envelope fields (`rev`/`metrics`/`running`/`relay_statuses`/
//! `logical_interests`/`wire_subscriptions`/`logs`/`last_error_toast`, ADR-0044
//! Tier-3) are written unconditionally onto every `SnapshotFrame`, so their
//! host fallbacks are trivially safe and are out of this map-keyed gate's scope.

use std::collections::BTreeSet;
use std::ffi::CString;

use nmp_ffi::{nmp_app_free, nmp_app_new, NmpApp};

use super::super::{
    nmp_app_chirp_close_group_discovery, nmp_app_chirp_open_group_discovery,
    nmp_app_chirp_register, nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list,
    nmp_app_chirp_register_group_events, nmp_app_chirp_unregister,
};

mod registry_coverage;

/// THE GATE: bootstrap the full Chirp projection surface and assert every
/// typed sidecar key is registered (generic lane deleted per A6 / PR #1515).
///
/// The generic `serde_json::Value` lane has been eliminated (A6 rule).
/// This gate now verifies the typed-sidecar surface is fully registered:
/// every registered typed projection key is surfaced through the typed path.
#[test]
fn every_generic_projection_key_has_a_typed_sidecar() {
    let app = nmp_app_new();
    assert!(!app.is_null());
    let mut handle: *mut super::super::ChirpHandle = std::ptr::null_mut();
    nmp_app_chirp_register(app, std::ptr::null(), &mut handle);
    assert!(!handle.is_null());
    nmp_app_chirp_register_dm_inbox(app);
    // follow_list and group_discovery accept an optional pubkey / relay-url
    // pointer; pass null to use the default (no active pubkey / no relay).
    nmp_app_chirp_register_follow_list(app, std::ptr::null());
    // open_group_discovery requires a real relay URL; null returns null (D6).
    let discovery_relay = CString::new("wss://groups.example.com").unwrap();
    let discovery_handle = nmp_app_chirp_open_group_discovery(app, discovery_relay.as_ptr());
    let group_request = CString::new(
        r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"abcd"},"kinds":[9,11]}"#,
    )
    .unwrap();
    nmp_app_chirp_register_group_events(app, group_request.as_ptr());

    let app_ref: &NmpApp = unsafe { &*app };
    // The generic lane is deleted (rule A6). Use registered_typed_projection_keys()
    // to get the full registered set without running closures (closures for
    // optional features return None when they have no data).
    let typed_keys: BTreeSet<String> = app_ref
        .registered_typed_projection_keys()
        .into_iter()
        .collect();

    // Guard against a vacuous pass: the full Chirp surface must register a
    // non-trivial typed key space.
    assert!(
        typed_keys.len() >= 3,
        "bootstrap registered only {} typed projection(s) ({typed_keys:?}) — \
         too few for the gate to be meaningful; the test harness is not \
         exercising the real Chirp registration surface",
        typed_keys.len()
    );

    // Close the discovery handle before freeing the app (D6 teardown).
    if !discovery_handle.is_null() {
        nmp_app_chirp_close_group_discovery(discovery_handle);
    }
    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
