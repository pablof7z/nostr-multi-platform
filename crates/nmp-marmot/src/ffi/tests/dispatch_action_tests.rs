use super::*;
use crate::projection::action::{MARMOT_ACTION_NAMESPACE, MarmotActionModule};

// ── ADR-0025 retirement / dispatch_action → MarmotProtocolCommand ───────
//
// The substrate-generic Marmot dispatch seam (the SOLE host entry point
// after ADR-0025 PR 3, 2026-05-23, deleted the legacy bespoke
// `nmp_marmot_dispatch` C-ABI symbol). The proof points:
//
//   1. `MarmotActionModule` registered against `NmpApp::register_action`
//      emits a typed `MarmotProtocolCommand` through `ActorCommand::Protocol`;
//      no host-op JSON bridge or raw `NmpApp` pointer is involved.
//   2. Both the host (generic) seam and the in-process Rust-native seam
//      (`MarmotHandle::dispatch` / direct `ops::dispatch`) share ONE
//      `MarmotProjection` — a dispatch through the generic path mutates
//      state visible to a subsequent read through the Rust-native path.
//      This is the property the ADR-0025 PR 3 deletion relied on, and the
//      property a future second Marmot host (post-Chirp) must continue to
//      satisfy.

fn dispatch_marmot_action(app: *mut nmp_ffi::NmpApp, envelope_json: &str) -> String {
    let namespace_c = CString::new(MARMOT_ACTION_NAMESPACE).unwrap();
    let envelope_c = CString::new(envelope_json).unwrap();
    let out_ptr = nmp_ffi::nmp_app_dispatch_action(app, namespace_c.as_ptr(), envelope_c.as_ptr());
    assert!(
        !out_ptr.is_null(),
        "dispatch_action must return a non-null envelope (D6)"
    );
    // SAFETY: the dispatcher returns a freshly-allocated NUL-terminated
    // string the caller must release via `nmp_free_string`.
    let out = unsafe { CStr::from_ptr(out_ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_ffi::nmp_free_string(out_ptr);
    let parsed: serde_json::Value = serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("dispatch return must be valid JSON; got `{out}`: {e}"));
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("dispatch envelope must carry a correlation_id; got: {out}"));
    assert_eq!(
        id.len(),
        32,
        "correlation_id must be 32 hex chars; got: {id}"
    );
    id.to_string()
}

fn wait_for_projection_state<T>(
    rx: &Receiver<Vec<u8>>,
    proj: &MarmotProjection,
    mut observe: impl FnMut(crate::projection::payload::MarmotSnapshot) -> Option<T>,
) -> T {
    if let Some(value) = observe(proj.snapshot(1_000)) {
        return value;
    }
    loop {
        rx.recv_timeout(Duration::from_secs(5))
            .expect("actor must emit an update frame");
        if let Some(value) = observe(proj.snapshot(1_000)) {
            return value;
        }
    }
}

fn wait_for_failed_action_stage(
    rx: &Receiver<Vec<u8>>,
    correlation_id: &str,
) -> nmp_core::typed_projections::LifecycleEntryRow {
    loop {
        let bytes = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("actor must emit an action lifecycle frame");
        let Ok(typed) = decode_snapshot_typed_projections(&bytes) else {
            continue;
        };
        let Some(sidecar) = typed.iter().find(|t| t.key == ACTION_LIFECYCLE_SCHEMA_ID) else {
            continue;
        };
        let Ok(model) = decode_action_lifecycle(&sidecar.payload) else {
            continue;
        };
        if let Some(row) = model
            .recent_terminal
            .into_iter()
            .find(|row| row.correlation_id == correlation_id && row.stage == "failed")
        {
            return row;
        }
    }
}

/// End-to-end proof of the dispatch_action → MarmotProtocolCommand path.
///
/// Builds the EXACT wiring `register_with_keys` does (minus the C-ABI
/// shell) directly on a fresh `NmpApp`:
///
/// * register `MarmotActionModule` against the action registry;
/// * give the shared projection the actor sender used for runtime commands.
///
/// Then dispatches a typed Marmot action JSON body through
/// `nmp_app_dispatch_action("nmp.marmot", action_json)` and asserts:
///
/// * the dispatcher returns a `correlation_id` (the action was accepted);
/// * the `MarmotProjection::snapshot` reflects the published key package
///   (the protocol command ran and mutated shared state — the SAME state the
///   Rust-native [`MarmotHandle::dispatch`] accessor mutates, and the
///   SAME state the legacy bespoke `nmp_marmot_dispatch` symbol used to
///   mutate before ADR-0025 PR 3 deleted it).
#[test]
fn dispatch_action_nmp_marmot_routes_to_projection_via_protocol_command() {
    let _capture_guard = ACTION_FRAME_CAPTURE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let rx = install_action_frame_capture();
    let alice_keys = Keys::generate();
    let proj = Arc::new(MarmotProjection::new(in_memory(alice_keys.clone()), None));

    let app = nmp_ffi::nmp_app_new();
    // SAFETY: nmp_app_new never returns null; pointer is valid until nmp_app_free.
    let app_mut = unsafe { &mut *app };

    // The two-line wiring `register_with_keys` performs for the
    // dispatch_action seam:
    proj.set_actor_sender(app_mut.actor_sender());
    let _ = app_mut.register_action(MarmotActionModule::new(Arc::clone(&proj)));
    nmp_ffi::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(capture_action_frame));
    nmp_ffi::nmp_app_start(app, 256, 4);

    // Dispatch the typed action JSON through the generic seam.
    let envelope_json = r#"{"op":"publish_key_package","relays":["wss://t.relay"]}"#;
    let _id = dispatch_marmot_action(app, envelope_json);
    let published = wait_for_projection_state(&rx, &proj, |snap| {
        snap.key_package.published.then_some(true)
    });
    assert!(
        published,
        "dispatch_action(nmp.marmot, publish_key_package) must route through \
         MarmotProtocolCommand and mutate the projection state visible to snapshot \
         (the SAME state MarmotHandle::dispatch mutates — i.e. the SAME state \
         the legacy bespoke nmp_marmot_dispatch symbol used to mutate, pre-PR-3)"
    );

    uninstall_action_frame_capture();
    nmp_ffi::nmp_app_free(app);
}

/// Parity test: the host (generic `dispatch_action`) seam and the
/// in-process Rust-native seam (direct `projection::ops::dispatch`, the
/// same code path `MarmotHandle::dispatch` reaches) mutate ONE shared
/// `MarmotProjection`. A `create_group` through `dispatch_action`
/// produces a group that a subsequent in-process `ops::dispatch` read
/// sees — no duplicate state store, no parallel mutex. This was the
/// precondition ADR-0025 PR 3 relied on when deleting the legacy bespoke
/// `nmp_marmot_dispatch` symbol; it remains the precondition the
/// REPL/TUI tests rely on now that the Rust-native accessor is the
/// substitute for the deleted C symbol.
#[test]
fn dispatch_action_and_bespoke_dispatch_share_one_projection() {
    let _capture_guard = ACTION_FRAME_CAPTURE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let rx = install_action_frame_capture();
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let charlie_keys = Keys::generate();
    let bob = in_memory(bob_keys.clone());
    let bob_kp_json = bob
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob kp")
        .event_30443
        .as_json();
    let charlie = in_memory(charlie_keys.clone());
    let charlie_kp_json = charlie
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("charlie kp")
        .event_30443
        .as_json();

    let proj = Arc::new(MarmotProjection::new(in_memory(alice_keys.clone()), None));

    let app = nmp_ffi::nmp_app_new();
    // SAFETY: nmp_app_new never returns null.
    let app_mut = unsafe { &mut *app };
    proj.set_actor_sender(app_mut.actor_sender());
    let _ = app_mut.register_action(MarmotActionModule::new(Arc::clone(&proj)));
    nmp_ffi::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(capture_action_frame));
    nmp_ffi::nmp_app_start(app, 256, 4);

    // Generic seam: create the group via dispatch_action.
    let envelope = json!({
        "op": "create_group",
        "name": "PR 1 parity",
        "description": "shared projection proof",
        "relays": ["wss://t.relay"],
        "invitee_npubs": [bob_keys.public_key().to_hex()],
        "signed_key_package_events_json": [bob_kp_json],
    })
    .to_string();
    let returned_id = dispatch_marmot_action(app, &envelope);
    let group_id_hex = wait_for_projection_state(&rx, &proj, |snap| {
        snap.groups.first().map(|g| g.id_hex.clone())
    });

    let invite = json!({
        "op": "invite",
        "group_id_hex": &group_id_hex,
        "relays": ["wss://t.relay"],
        "invitee_npubs": [charlie_keys.public_key().to_hex()],
        "signed_key_package_events_json": [charlie_kp_json],
    })
    .to_string();
    let invite_id = dispatch_marmot_action(app, &invite);
    let charlie_hex = charlie_keys.public_key().to_hex();
    wait_for_projection_state(&rx, &proj, |snap| {
        snap.groups
            .iter()
            .find(|g| {
                g.id_hex == group_id_hex
                    && g.member_count >= 3
                    && g.members.iter().any(|m| m == &charlie_hex)
            })
            .map(|_| ())
    });

    // In-process Rust-native seam (via ops::dispatch — the SAME entry
    // point MarmotHandle::dispatch reaches, and the SAME entry point the
    // legacy bespoke `nmp_marmot_dispatch` symbol used to reach pre-PR-3):
    // send a message into the just-created group. If the generic seam and
    // the Rust-native seam were separate stores, this would fail with
    // `unknown group_id`.
    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "send",
                    "group_id_hex": &group_id_hex,
                    "text": "parity proof",
                }),
                1_001,
                None,
            )
        })
        .expect("projection mutex should not be poisoned");
    assert_eq!(
        r["ok"],
        json!(true),
        "the in-process Rust-native seam (ops::dispatch / \
         MarmotHandle::dispatch) must see the group created through the \
         generic dispatch_action seam: {r}"
    );

    assert_eq!(returned_id.len(), 32);
    assert_eq!(invite_id.len(), 32);
    uninstall_action_frame_capture();
    nmp_ffi::nmp_app_free(app);
}

#[test]
fn dispatch_action_failure_records_typed_failed_stage() {
    let _capture_guard = ACTION_FRAME_CAPTURE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let rx = install_action_frame_capture();
    let alice_keys = Keys::generate();
    let proj = Arc::new(MarmotProjection::new(in_memory(alice_keys), None));

    let app = nmp_ffi::nmp_app_new();
    // SAFETY: nmp_app_new never returns null.
    let app_mut = unsafe { &mut *app };
    proj.set_actor_sender(app_mut.actor_sender());
    let _ = app_mut.register_action(MarmotActionModule::new(Arc::clone(&proj)));
    nmp_ffi::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(capture_action_frame));
    nmp_ffi::nmp_app_start(app, 256, 4);

    let cid = dispatch_marmot_action(
        app,
        r#"{"op":"send","group_id_hex":"not-hex","text":"must fail"}"#,
    );
    let row = wait_for_failed_action_stage(&rx, &cid);
    assert_eq!(row.correlation_id, cid);
    assert_eq!(row.stage, "failed");
    assert!(
        row.reason
            .as_deref()
            .unwrap_or_default()
            .contains("group_id_hex"),
        "failed stage should carry the semantic Marmot error: {row:?}"
    );

    uninstall_action_frame_capture();
    nmp_ffi::nmp_app_free(app);
}
