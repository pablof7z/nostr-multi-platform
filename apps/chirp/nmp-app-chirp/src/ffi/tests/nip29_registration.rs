//! NIP-29 FFI registration wiring proofs: group-chat and group-discovery
//! lifecycle.
//!
//! Extracted from `ffi/tests/nip29.rs` to keep each file under the 500-LOC
//! cap (AGENTS.md). Covers the C-ABI entry-point surface for:
//!   - `nmp_app_chirp_open_group_discovery` / `close_group_discovery`
//!   - `nmp_app_chirp_register_group_chat` (parse-gate + idempotency)

use std::ffi::CString;

use nmp_ffi::{nmp_app_free, nmp_app_new};
use nmp_nip29::group_id::GroupId;

use super::super::{
    nmp_app_chirp_close_group_discovery, nmp_app_chirp_open_group_discovery,
    nmp_app_chirp_register_group_chat,
};

/// THE DISCOVERY REGISTRATION WIRING PROOF: `nmp_app_chirp_open_group_discovery`
/// registers a `DiscoveredGroupsProjection` against `app` for a well-formed
/// relay URL — it runs to completion (event-observer + snapshot-projection
/// registration) without panicking and returns a non-null handle. The snapshot
/// closure surfacing under `"nmp.nip29.discovered_groups"` is proven end-to-end
/// by the generic seam tests in `nmp-core` and the projection's own tests in
/// `nmp-nip29`. The returned handle must be closed before `nmp_app_free`.
#[test]
fn open_group_discovery_runs_for_well_formed_relay_url() {
    let app = nmp_app_new();
    let relay = CString::new("wss://groups.example.com").unwrap();
    let handle = nmp_app_chirp_open_group_discovery(app, relay.as_ptr());
    assert!(
        !handle.is_null(),
        "open_group_discovery must return a non-null handle for a well-formed relay URL"
    );
    nmp_app_chirp_close_group_discovery(handle);
    nmp_app_free(app);
}

/// D6: a null `app`, a null `host_relay_url`, and an empty `host_relay_url`
/// all degrade to a null return — the function must never panic across the
/// FFI boundary.
#[test]
fn open_group_discovery_null_and_empty_input_are_silent_noops() {
    let relay = CString::new("wss://groups.example.com").unwrap();
    // Null app — must not dereference; returns null.
    let h = nmp_app_chirp_open_group_discovery(std::ptr::null_mut(), relay.as_ptr());
    assert!(h.is_null(), "null app must return null handle");

    let app = nmp_app_new();
    // Null host_relay_url — silent return.
    let h = nmp_app_chirp_open_group_discovery(app, std::ptr::null());
    assert!(h.is_null(), "null relay_url must return null handle");
    // Empty string — silent return.
    let empty = CString::new("").unwrap();
    let h = nmp_app_chirp_open_group_discovery(app, empty.as_ptr());
    assert!(h.is_null(), "empty relay_url must return null handle");
    nmp_app_free(app);
}

/// THE GROUP-ID WIRE-SHAPE CONTRACT: the JSON shape documented on
/// `nmp_app_chirp_register_group_chat` — `{"host_relay_url":…,
/// "local_id":…}` — is exactly what `GroupId`'s serde derive accepts.
/// This is the contract a Swift caller depends on: a body of any other
/// shape is rejected by the `serde_json::from_str::<GroupId>` parse gate
/// inside the function and the registration silently no-ops (D6).
#[test]
fn register_group_chat_group_id_wire_shape_matches_serde() {
    let parsed: GroupId =
        serde_json::from_str(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#)
            .expect("documented group_id_json shape must deserialize to GroupId");
    assert_eq!(parsed.host_relay_url, "wss://groups.example.com");
    assert_eq!(parsed.local_id, "room");

    // A JSON object missing the required fields is NOT a `GroupId` — the
    // parse gate rejects it, so the function returns without registering.
    assert!(
        serde_json::from_str::<GroupId>(r#"{"not":"a group id"}"#).is_err(),
        "a wrong-shape body must fail the GroupId parse gate"
    );
}

/// THE GROUP-CHAT WIRING PROOF: `nmp_app_chirp_register_group_chat`
/// registers a `GroupChatProjection` against `app` for a well-formed
/// group id — it runs to completion (event-observer + snapshot-projection
/// registration) without panicking. The snapshot closure surfacing under
/// `"nmp.nip29.group_chat"` is proven end-to-end by the generic seam tests in
/// `nmp-core` (`snapshot_registry_tests.rs`) and the projection's own
/// tests in `nmp-nip29`; this asserts the Chirp-side wiring call is sound.
#[test]
fn register_group_chat_runs_for_well_formed_group() {
    let app = nmp_app_new();
    let group =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#).unwrap();
    // Must register both halves (observer + snapshot projection) without
    // panicking across the FFI boundary.
    nmp_app_chirp_register_group_chat(app, group.as_ptr());
    nmp_app_free(app);
}

/// THE IDEMPOTENCY PROOF — group-chat variant. Same shape as the
/// DM-inbox test: two consecutive `register_group_chat` calls leave
/// exactly one `KernelEventObserverId` in the per-app
/// `singleton_event_observer_id` slot, with the second register's id
/// distinct from the first (proving the slot was overwritten and the
/// previous observer was unregistered against the kernel).
#[test]
fn register_group_chat_is_idempotent_on_re_invoke() {
    let app = nmp_app_new();
    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for the
    // duration of this test.
    let app_ref = unsafe { &*app };

    assert!(
        app_ref.swap_singleton_event_observer(None).is_none(),
        "slot must start empty (no group chat registered yet)"
    );

    let group_a =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room-a"}"#)
            .unwrap();
    let group_b =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room-b"}"#)
            .unwrap();

    // First registration.
    nmp_app_chirp_register_group_chat(app, group_a.as_ptr());
    let id1 = app_ref
        .swap_singleton_event_observer(None)
        .expect("first register must install a kernel-observer id in the per-app slot");
    let prev = app_ref.swap_singleton_event_observer(Some(id1));
    assert!(prev.is_none(), "we just swap-took, slot was empty");

    // Second registration with a different group — the multi-screen
    // navigation case that previously leaked the prior observer.
    nmp_app_chirp_register_group_chat(app, group_b.as_ptr());
    let id2 = app_ref
        .swap_singleton_event_observer(None)
        .expect("second register must install a fresh id in the per-app slot");
    assert_ne!(
        id1, id2,
        "second register must produce a fresh kernel-observer id (got {id1:?} both times)"
    );

    app_ref.unregister_event_observer(id2);
    nmp_app_free(app);
}

/// D6: a null `app`, a null `group_id_json`, and a malformed
/// `group_id_json` (valid JSON, wrong fields) all degrade to a silent
/// no-op — the function must never panic across the FFI boundary.
#[test]
fn register_group_chat_null_and_malformed_input_are_silent_noops() {
    let group =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#).unwrap();
    // Null app — must not dereference.
    nmp_app_chirp_register_group_chat(std::ptr::null_mut(), group.as_ptr());

    let app = nmp_app_new();
    // Null group id — silent return.
    nmp_app_chirp_register_group_chat(app, std::ptr::null());
    // Malformed JSON shape — fails the `GroupId` parse gate, silent return.
    let bad = CString::new(r#"{"not":"a group id"}"#).unwrap();
    nmp_app_chirp_register_group_chat(app, bad.as_ptr());
    // Non-JSON garbage — also fails the parse gate, silent return.
    let garbage = CString::new("not json at all").unwrap();
    nmp_app_chirp_register_group_chat(app, garbage.as_ptr());
    nmp_app_free(app);
}
