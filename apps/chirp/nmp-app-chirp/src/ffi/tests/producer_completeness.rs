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
    nmp_app_chirp_register, nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list,
    nmp_app_chirp_register_group_chat, nmp_app_chirp_register_group_discovery,
    nmp_app_chirp_unregister,
};

/// THE GATE: bootstrap the full Chirp projection surface, then assert every
/// generic `Value` projection key has a typed sidecar under the same key.
#[test]
fn every_generic_projection_key_has_a_typed_sidecar() {
    let app = nmp_app_new();
    assert!(!app.is_null());

    let viewer = CString::new("aa".repeat(32)).unwrap();
    let mut handle = std::ptr::null_mut();
    let status = nmp_app_chirp_register(app, viewer.as_ptr(), &mut handle);
    assert_eq!(
        status,
        super::super::NmpRegisterStatus::Ok as u32,
        "register with valid viewer_pubkey must succeed"
    );
    assert!(!handle.is_null());

    // Install every projection-bearing subsystem so the shared registry holds
    // the full Chirp key space (each `register_*` installs a generic+typed
    // PAIR; the actor side — bunker_handshake / nip46_onboarding — shares the
    // SAME registry `Arc<Mutex<…>>`, lib.rs:783-784, so it is covered too).
    nmp_app_chirp_register_dm_inbox(app);
    let active = CString::new("aa".repeat(32)).unwrap();
    nmp_app_chirp_register_follow_list(app, active.as_ptr());
    let host = CString::new("wss://groups.example.com").unwrap();
    nmp_app_chirp_register_group_discovery(app, host.as_ptr());
    let group_id = CString::new(r#"{"host":"wss://groups.example.com","id":"abcd"}"#).unwrap();
    nmp_app_chirp_register_group_chat(app, group_id.as_ptr());

    let app_ref: &NmpApp = unsafe { &*app };
    // A generic projection that emits `Value::Null` carries NO data — the host
    // decodes JSON-null and a typed-absent key to the *same* `nil` (e.g.
    // `"wallet"` while no wallet is connected: the generic closure emits
    // `Value::Null`, the paired typed closure omits the key, and
    // `typedWallet ?? snapshot?.walletStatus` yields `nil` either way). Only a
    // generic key carrying a non-null value needs a typed counterpart, so null
    // values are excluded before the containment check.
    let json_keys: BTreeSet<String> = app_ref
        .run_snapshot_projections()
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, _)| key)
        .collect();
    let typed_keys: BTreeSet<String> = app_ref
        .run_typed_snapshot_projections()
        .into_iter()
        .map(|data| data.key)
        .collect();

    let uncovered: Vec<&String> = json_keys.difference(&typed_keys).collect();
    assert!(
        uncovered.is_empty(),
        "generic `payload:Value` projection keys with NO typed sidecar — a host's \
         `typed<K> ?? snapshot?.<k>` fallback silently serves these from JSON, so \
         removing the fallback (and ultimately `payload:Value`) would lose them. \
         Type each before the fallback-removal PR:\n  uncovered: {uncovered:?}\n  \
         generic keys: {json_keys:?}\n  typed keys: {typed_keys:?}"
    );

    // Guard against a vacuous pass: if the bootstrap emitted no generic
    // projections at all, the subset check above is trivially (and uselessly)
    // true. The full Chirp surface must register a non-trivial key space.
    assert!(
        json_keys.len() >= 3,
        "bootstrap registered only {} generic projection(s) ({json_keys:?}) — \
         too few for the gate to be meaningful; the test harness is not \
         exercising the real Chirp registration surface",
        json_keys.len()
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
