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
    nmp_app_chirp_register_group_chat, nmp_app_chirp_unregister_group_chat,
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

/// THE GROUP-CHAT WIRING PROOF: `nmp_app_chirp_register_group_chat` opens a
/// hydrating `GroupChatProjection` read view against `app` for a well-formed
/// group id — it runs to completion (typed sidecar registration + muted
/// observer + relay-pinned observed interest) without panicking, and the
/// `"nmp.nip29.group_chat"` snapshot key is synchronously registered. The
/// hydration end-to-end is proven by the `nmp-ffi` integration tests; this
/// asserts the Chirp-side delegation is sound, and that `unregister` removes
/// the key again (#2088 teardown).
#[test]
fn register_group_chat_runs_for_well_formed_group() {
    let app = nmp_app_new();
    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for this test.
    let app_ref = unsafe { &*app };
    let group =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#).unwrap();
    nmp_app_chirp_register_group_chat(app, group.as_ptr());
    assert!(
        app_ref
            .registered_typed_projection_keys()
            .iter()
            .any(|k| k == "nmp.nip29.group_chat"),
        "register_group_chat must synchronously register the group_chat snapshot key"
    );
    // Teardown removes the key (no stale chat log after screen dismissal).
    nmp_app_chirp_unregister_group_chat(app);
    assert!(
        !app_ref
            .registered_typed_projection_keys()
            .iter()
            .any(|k| k == "nmp.nip29.group_chat"),
        "unregister_group_chat must remove the group_chat snapshot key"
    );
    nmp_app_free(app);
}

/// THE IDEMPOTENCY PROOF — group-chat variant. Two consecutive
/// `register_group_chat` calls (the multi-screen navigation case that
/// previously leaked the prior observer) leave EXACTLY ONE
/// `"nmp.nip29.group_chat"` snapshot projection registered: the singleton open
/// path closes the prior hydrating session before installing the replacement,
/// so there is no leak and no duplicate key.
#[test]
fn register_group_chat_is_idempotent_on_re_invoke() {
    let app = nmp_app_new();
    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for the
    // duration of this test.
    let app_ref = unsafe { &*app };

    let key_count = |a: &nmp_ffi::NmpApp| {
        a.registered_typed_projection_keys()
            .iter()
            .filter(|k| *k == "nmp.nip29.group_chat")
            .count()
    };

    assert_eq!(key_count(app_ref), 0, "no group chat registered yet");

    let group_a =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room-a"}"#)
            .unwrap();
    let group_b =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room-b"}"#)
            .unwrap();

    nmp_app_chirp_register_group_chat(app, group_a.as_ptr());
    assert_eq!(key_count(app_ref), 1, "first register installs one view");

    // Second registration with a different group — re-open must close the
    // prior session first, leaving exactly one live group_chat view.
    nmp_app_chirp_register_group_chat(app, group_b.as_ptr());
    assert_eq!(
        key_count(app_ref),
        1,
        "re-register must keep exactly one group_chat view (no leak, no duplicate)"
    );

    nmp_app_chirp_unregister_group_chat(app);
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
