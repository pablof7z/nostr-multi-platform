//! NIP-29 FFI registration wiring proofs: group-events and group-discovery
//! lifecycle.
//!
//! Extracted from `ffi/tests/nip29.rs` to keep each file under the 500-LOC
//! cap (AGENTS.md). Covers the C-ABI entry-point surface for:
//!   - `nmp_app_chirp_open_group_discovery` / `close_group_discovery`
//!   - `nmp_app_chirp_register_group_events` (parse-gate + idempotency)

use std::ffi::CString;

use nmp_ffi::{nmp_app_free, nmp_app_new};

use super::super::{
    nmp_app_chirp_close_group_discovery, nmp_app_chirp_open_group_discovery,
    nmp_app_chirp_register_group_events, nmp_app_chirp_unregister_group_events,
};

/// A well-formed `{group, kinds}` request body for a chat view (kinds `[9, 11]`).
fn chat_request(local_id: &str) -> CString {
    CString::new(format!(
        r#"{{"group":{{"host_relay_url":"wss://groups.example.com","local_id":"{local_id}"}},"kinds":[9,11]}}"#
    ))
    .unwrap()
}

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

/// THE REQUEST WIRE-SHAPE CONTRACT: the JSON shape documented on
/// `nmp_app_chirp_register_group_events` — `{"group":{…},"kinds":[…]}` — is
/// exactly what the function's `GroupEventsRequest` parse gate accepts. NIP-29
/// owns only the `["h", local_id]` routing (issue #2187); the consumer declares
/// both the group AND the kinds. A **missing `kinds`** field is rejected (an old
/// `GroupId`-only body must not silently widen into a broad all-kinds read).
#[test]
fn register_group_events_request_wire_shape_rejects_missing_kinds() {
    use serde::Deserialize;
    use nmp_nip29::group_id::GroupId;

    #[derive(Deserialize)]
    struct Probe {
        group: GroupId,
        kinds: Vec<u32>,
    }

    let parsed: Probe = serde_json::from_str(
        r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"},"kinds":[9,11]}"#,
    )
    .expect("documented {group,kinds} shape must deserialize");
    assert_eq!(parsed.group.host_relay_url, "wss://groups.example.com");
    assert_eq!(parsed.group.local_id, "room");
    assert_eq!(parsed.kinds, vec![9, 11]);

    // A body with the group but NO `kinds` field is invalid — the parse gate
    // rejects it, so the function returns without registering.
    assert!(
        serde_json::from_str::<Probe>(
            r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"}}"#
        )
        .is_err(),
        "a body missing `kinds` must fail the request parse gate"
    );
}

/// THE GROUP-EVENTS WIRING PROOF: `nmp_app_chirp_register_group_events` opens a
/// hydrating `GroupEventsProjection` read view against `app` for a well-formed
/// `{group, kinds}` request — it runs to completion (typed sidecar registration
/// + muted observer + relay-pinned observed interest) without panicking, and the
/// `"nmp.nip29.group_events"` snapshot key is synchronously registered. The
/// hydration end-to-end is proven by the `nmp-ffi` integration tests; this
/// asserts the Chirp-side delegation is sound, and that `unregister` removes
/// the key again (#2088 teardown).
#[test]
fn register_group_events_runs_for_well_formed_request() {
    let app = nmp_app_new();
    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for this test.
    let app_ref = unsafe { &*app };
    let request = chat_request("room");
    nmp_app_chirp_register_group_events(app, request.as_ptr());
    assert!(
        app_ref
            .registered_typed_projection_keys()
            .iter()
            .any(|k| k == "nmp.nip29.group_events"),
        "register_group_events must synchronously register the group_events snapshot key"
    );
    // Teardown removes the key (no stale event log after screen dismissal).
    nmp_app_chirp_unregister_group_events(app);
    assert!(
        !app_ref
            .registered_typed_projection_keys()
            .iter()
            .any(|k| k == "nmp.nip29.group_events"),
        "unregister_group_events must remove the group_events snapshot key"
    );
    nmp_app_free(app);
}

/// THE IDEMPOTENCY PROOF — group-events variant. Two consecutive
/// `register_group_events` calls (the multi-screen navigation case that
/// previously leaked the prior observer) leave EXACTLY ONE
/// `"nmp.nip29.group_events"` snapshot projection registered: the singleton open
/// path closes the prior hydrating session before installing the replacement,
/// so there is no leak and no duplicate key.
#[test]
fn register_group_events_is_idempotent_on_re_invoke() {
    let app = nmp_app_new();
    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for the
    // duration of this test.
    let app_ref = unsafe { &*app };

    let key_count = |a: &nmp_ffi::NmpApp| {
        a.registered_typed_projection_keys()
            .iter()
            .filter(|k| *k == "nmp.nip29.group_events")
            .count()
    };

    assert_eq!(key_count(app_ref), 0, "no group events view registered yet");

    let request_a = chat_request("room-a");
    let request_b = chat_request("room-b");

    nmp_app_chirp_register_group_events(app, request_a.as_ptr());
    assert_eq!(key_count(app_ref), 1, "first register installs one view");

    // Second registration with a different group — re-open must close the
    // prior session first, leaving exactly one live group_events view.
    nmp_app_chirp_register_group_events(app, request_b.as_ptr());
    assert_eq!(
        key_count(app_ref),
        1,
        "re-register must keep exactly one group_events view (no leak, no duplicate)"
    );

    nmp_app_chirp_unregister_group_events(app);
    nmp_app_free(app);
}

/// D6: a null `app`, a null `request_json`, a malformed `request_json` (valid
/// JSON, wrong fields), and a body missing `kinds` all degrade to a silent
/// no-op — the function must never panic across the FFI boundary.
#[test]
fn register_group_events_null_and_malformed_input_are_silent_noops() {
    let request = chat_request("room");
    // Null app — must not dereference.
    nmp_app_chirp_register_group_events(std::ptr::null_mut(), request.as_ptr());

    let app = nmp_app_new();
    // Null request — silent return.
    nmp_app_chirp_register_group_events(app, std::ptr::null());
    // Malformed JSON shape — fails the request parse gate, silent return.
    let bad = CString::new(r#"{"not":"a request"}"#).unwrap();
    nmp_app_chirp_register_group_events(app, bad.as_ptr());
    // A body with the group but no `kinds` — invalid, silent return.
    let missing_kinds =
        CString::new(r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"}}"#)
            .unwrap();
    nmp_app_chirp_register_group_events(app, missing_kinds.as_ptr());
    // Non-JSON garbage — also fails the parse gate, silent return.
    let garbage = CString::new("not json at all").unwrap();
    nmp_app_chirp_register_group_events(app, garbage.as_ptr());
    nmp_app_free(app);
}
