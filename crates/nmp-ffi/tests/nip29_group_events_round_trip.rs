//! NIP-29 group-chat end-to-end round-trip.
//!
//! Validates the full publish + receive stack at the NMP layer — zero Chirp
//! symbols, no architectural shortcuts.
//!
//! ## Publish side
//!
//! `nmp_app_dispatch_action_bytes(app, envelope)` (ADR-0064 typed byte doorway)
//! routes through the registered `PublishGroupEventAction` module: the typed
//! validator runs synchronously and the executor enqueues
//! `ActorCommand::PublishUnsignedEventToRelays` pinned to the group's host
//! relay. The test encodes a typed `PublishGroupEventInput` (a kind:9 chat
//! message)
//! [`ActionPayload`](nmp_core::substrate::ActionPayload), wraps it in an open
//! [`DispatchEnvelope`](nmp_core::dispatch_envelope) with a host-minted
//! `correlation_id`, and dispatches the finished bytes — the JSON
//! `nmp_app_dispatch_action` doorway is retired (#1996). Actions are registered
//! by `nmp_nip29::register::register_actions` — the same call any host (Chirp, a
//! TUI, a REPL) makes at startup.
//!
//! ## Receive side
//!
//! A well-formed kind:9 event carrying `["h", local_id]` is injected with relay
//! provenance via `ActorCommand::IngestPreVerifiedEventsForRelay`. This matches
//! the path a relay worker follows when it delivers a verified event into the
//! actor loop. The actor fans it out through `notify_event_observers`;
//! `GroupEventsProjection` (opened by a NIP-29 typed read session, #2088)
//! accumulates it and surfaces it under
//! `projections["nmp.nip29.group_events"]["events"]` on the next snapshot tick.
//! The test reads that snapshot via `test_app_set_update_callback` — the same
//! path any shell (iOS KernelBridge, a TUI, a web bridge) uses.
//!
//! ## Why no real relay?
//!
//! `IngestPreVerifiedEvents` is the exact path a relay worker takes after
//! signature verification — the projection code cannot distinguish relay-
//! delivered from injected events. A two-instance relay-bridged test is left
//! for when that harness is available.

use std::ffi::c_void;
use std::sync::Mutex;
use std::time::Duration;

use nmp_core::actor::{ActorCommand, TestSupportCommand};
use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_ffi::NmpApp;
use nmp_native_runtime::{dispatch_action_bytes_typed, DispatchOutcome, Nip29GroupEventsSession};
use nmp_nip29::action::PublishGroupEventInput;
use nmp_nip29::group_id::GroupId;
use nmp_nip29::register::register_actions;
use nmp_store::{RawEvent, VerifiedEvent};

/// Dispatch a typed `PublishGroupEventInput` through the ADR-0064 byte doorway
/// ([`nmp_app_dispatch_action_bytes`]) and return the result envelope JSON.
///
/// Mirrors the `apps/chirp/crates/nmp-app-chirp::dispatch_action_bytes_for` seam
/// (#1996): encode the typed [`ActionPayload`], wrap it in an open
/// [`DispatchEnvelope`](nmp_core::dispatch_envelope) with a host-minted
/// `correlation_id`, hand the finished bytes to the doorway, and copy the
/// returned C string. `nmp-nip29` cannot depend on `nmp-app-chirp` (that would
/// invert the crate stack), so the small encode-and-dispatch shape is inlined
/// here.
fn outcome_to_json(outcome: DispatchOutcome) -> String {
    match (outcome.correlation_id, outcome.error, outcome.code) {
        (Some(cid), None, None) => format!(r#"{{"correlation_id":{cid:?}}}"#),
        (Some(cid), Some(err), None) => {
            format!(r#"{{"correlation_id":{cid:?},"error":{err:?}}}"#)
        }
        (None, Some(err), Some(code)) => format!(r#"{{"error":{err:?},"code":{code:?}}}"#),
        (None, Some(err), None) => format!(r#"{{"error":{err:?}}}"#),
        _ => r#"{"error":"internal: malformed dispatch outcome"}"#.to_string(),
    }
}

fn dispatch_publish_group_event(app: &NmpApp, action: &PublishGroupEventInput) -> String {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let correlation_id = format!(
        "nip29-test-{}",
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let payload = action.encode();
    let envelope = encode_dispatch_envelope(
        &correlation_id,
        PublishGroupEventInput::SCHEMA_ID,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    outcome_to_json(dispatch_action_bytes_typed(app, &envelope))
}

// Tests that spin up NmpApp instances must be serialised: each spawns global
// actor threads that do not cleanly isolate across parallel test processes.
static SERIAL: Mutex<()> = Mutex::new(());

// Raw FlatBuffers frames collected by the update callback (decoded lazily by
// the poll helper — PR-B: the generic JSON payload no longer exists).
static SNAPSHOTS: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
const HOST_RELAY: &str = "wss://groups.example.com";

extern "C" fn collect_snapshot(_ctx: *mut c_void, bytes: *const u8, len: usize) {
    if bytes.is_null() {
        return;
    }
    // SAFETY: the FFI listener owns `bytes` for the duration of this call.
    let frame = unsafe { std::slice::from_raw_parts(bytes, len) };
    SNAPSHOTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(frame.to_vec());
}

type TestUpdateCallback = extern "C" fn(*mut c_void, *const u8, usize);

fn test_app_new() -> *mut NmpApp {
    Box::into_raw(Box::new(nmp_native_runtime::new_app()))
}

fn test_app_free(app: *mut NmpApp) {
    if !app.is_null() {
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}

fn test_app_set_update_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<TestUpdateCallback>,
) {
    let Some(app) = (unsafe { app.as_ref() }) else {
        return;
    };
    let listener = callback.map(|callback| {
        let context = context as usize;
        std::sync::Arc::new(move |bytes: &[u8]| {
            callback(context as *mut c_void, bytes.as_ptr(), bytes.len());
        }) as nmp_native_runtime::UpdateListener
    });
    app.set_update_listener(listener);
}

fn test_app_start(app: *mut NmpApp, visible_limit: u32, emit_hz: u32) {
    let Some(app) = (unsafe { app.as_ref() }) else {
        return;
    };
    app.start_runtime(visible_limit as usize, emit_hz);
}

/// Build a minimal kind:9 group-chat event for injection.
fn raw_chat_event(id: &str, author: &str, local_id: &str, ts: u64, content: &str) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at: ts,
        kind: 9,
        tags: vec![vec!["h".to_string(), local_id.to_string()]],
        content: content.to_string(),
        sig: "0".repeat(128),
    }
}

fn inject(app: *mut nmp_ffi::NmpApp, events: Vec<VerifiedEvent>) {
    // SAFETY: `app` is a valid pointer from `test_app_new` owned by the caller.
    let app_ref = unsafe { &*app };
    app_ref
        .actor_sender()
        .send(ActorCommand::TestSupport(
            TestSupportCommand::IngestPreVerifiedEventsForRelay {
                relay_url: HOST_RELAY.to_string(),
                events,
            },
        ))
        .expect("actor command channel must be open");
}

/// Poll `SNAPSHOTS` until a snapshot tick's typed `"nmp.nip29.group_events"`
/// sidecar contains a group-chat message with `content`, or the 3-second
/// deadline passes. PR-B: decodes the typed FlatBuffers sidecar via
/// `decode_group_events_snapshot` — the generic JSON payload no longer exists.
fn wait_for_group_message(content: &str) -> bool {
    use nmp_nip29::{decode_group_events_snapshot, GROUP_EVENTS_SCHEMA_ID};

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        {
            let snaps = SNAPSHOTS.lock().unwrap_or_else(|p| p.into_inner());
            for frame in snaps.iter() {
                let Ok(typed) = nmp_core::decode_snapshot_typed_projections(frame) else {
                    continue;
                };
                let found = typed
                    .iter()
                    .find(|t| t.key == GROUP_EVENTS_SCHEMA_ID)
                    .and_then(|t| decode_group_events_snapshot(&t.payload).ok())
                    .map(|snapshot| snapshot.events.iter().any(|m| m.content == content))
                    .unwrap_or(false);
                if found {
                    return true;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

// ── Publish side ─────────────────────────────────────────────────────────────

/// Proves the publish-side seam is live: `nmp_app_dispatch_action_bytes` routes
/// the typed `nmp.nip29.publish_group_event` payload through both the
/// `PublishGroupEventAction` module (typed validator → echoed `correlation_id`)
/// and executor (enqueues `PublishUnsignedEventToRelays` on the actor channel,
/// fire-and-forget).
///
/// Registered via `nmp_nip29::register::register_actions` — zero Chirp
/// symbols. Any host calls this same function at startup.
#[test]
fn publish_group_event_dispatch_returns_correlation_id() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let app = test_app_new();
    // SAFETY: `app` is a valid pointer from `test_app_new`; no other reference
    // aliases it at these call sites.
    register_actions(unsafe { &mut *app }).expect("NIP-29 actions register");

    let action = PublishGroupEventInput {
        group: GroupId::new(HOST_RELAY, "test-room"),
        kind: 9,
        content: "hello from TUI".to_string(),
        tags: Vec::new(),
    };
    let out = dispatch_publish_group_event(unsafe { &*app }, &action);

    let result: serde_json::Value = serde_json::from_str(&out).unwrap();
    // On the byte lane the correlation_id is HOST-supplied (ADR-0064 §4) and
    // echoed back verbatim; assert the doorway accepted (non-empty id, no error).
    let cid = result
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id in dispatch result, got: {result}"));
    assert!(
        cid.starts_with("nip29-test-"),
        "the doorway must echo the host-supplied correlation_id, got: {cid}"
    );

    // Malformed payload (non-routable group: empty host relay url) is rejected
    // by the typed module validator — the executor is never reached.
    let bad = PublishGroupEventInput {
        group: GroupId::new("", "test-room"),
        kind: 9,
        content: "x".to_string(),
        tags: Vec::new(),
    };
    let out2 = dispatch_publish_group_event(unsafe { &*app }, &bad);
    let result2: serde_json::Value = serde_json::from_str(&out2).unwrap();
    assert!(
        result2.get("error").is_some(),
        "dispatch with a non-routable group must be rejected by the typed validator: {result2}"
    );

    test_app_free(app);
}

// ── Receive side ─────────────────────────────────────────────────────────────

/// Proves the receive-side seam is live end-to-end:
///
/// 1. A NIP-29 group-events typed read session opens a hydrating `GroupEventsProjection` for
///    `"test-room"` as an `ObservedProjectionSink` (ingest) + snapshot projection
///    under `"nmp.nip29.group_events"` (output).
/// 2. A kind:9 event carrying `["h", "test-room"]` is injected via
///    `IngestPreVerifiedEvents` — the same actor path a relay worker uses.
/// 3. The `GroupEventsProjection` accumulates the event; on the next snapshot
///    tick the kernel serializes it under `projections["nmp.nip29.group_events"]`.
/// 4. The update callback (set via `test_app_set_update_callback`) receives the
///    JSON string — the same path any shell reads from.
/// 5. A decoy event for a different group must NOT appear.
#[test]
fn group_events_event_surfaces_via_kernel_snapshot_callback() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    SNAPSHOTS.lock().unwrap_or_else(|p| p.into_inner()).clear();

    let app = test_app_new();

    // Register the update callback before start so no snapshot tick is missed.
    test_app_set_update_callback(app, std::ptr::null_mut(), Some(collect_snapshot));

    // Declare projection-consumption intent like a real host (ADR-0053 / E4) so
    // `test_app_start` does not trip its forgotten-declaration guard in this
    // non-`test-support` integration-test build of nmp-ffi.
    unsafe { &*app }.consume_all_builtin_projections();

    // test_app_start sends ActorCommand::Start; the actor enters its main loop
    // and begins emitting snapshot ticks at emit_hz rate.
    test_app_start(app, 64, 8); // emit_hz=8 → ~125 ms cadence

    // Wire the GroupEventsProjection for "test-room".
    // SAFETY: `app` is a valid pointer from `test_app_new`, live for this block.
    let app_ref = unsafe { &*app };
    app_ref.open_nip29_group_events_session(Nip29GroupEventsSession::new(
        GroupId::new(HOST_RELAY, "test-room"),
        vec![9, 11],
    ));

    // Inject the target event: kind:9 with h-tag "test-room".
    let target = VerifiedEvent::from_raw_unchecked(raw_chat_event(
        &"a".repeat(64),
        &"b".repeat(64),
        "test-room",
        1_700_000_000,
        "hello from TUI",
    ));

    // Inject a decoy event for a different group — must NOT appear in the
    // projection snapshot for "test-room".
    let decoy = VerifiedEvent::from_raw_unchecked(raw_chat_event(
        &"c".repeat(64),
        &"d".repeat(64),
        "other-room",
        1_700_000_001,
        "should not appear",
    ));

    inject(app, vec![target, decoy]);

    // Wait up to 3 s for the snapshot to carry the target message.
    assert!(
        wait_for_group_message("hello from TUI"),
        "kind:9 event for 'test-room' must appear in the typed nmp.nip29.group_events sidecar within 3 s"
    );

    // Verify the decoy did NOT leak into the typed projection sidecar.
    {
        use nmp_nip29::{decode_group_events_snapshot, GROUP_EVENTS_SCHEMA_ID};

        let snaps = SNAPSHOTS.lock().unwrap_or_else(|p| p.into_inner());
        for frame in snaps.iter() {
            let Ok(typed) = nmp_core::decode_snapshot_typed_projections(frame) else {
                continue;
            };
            if let Some(snapshot) = typed
                .iter()
                .find(|t| t.key == GROUP_EVENTS_SCHEMA_ID)
                .and_then(|t| decode_group_events_snapshot(&t.payload).ok())
            {
                assert!(
                    !snapshot
                        .events
                        .iter()
                        .any(|m| m.content == "should not appear"),
                    "decoy event for 'other-room' must not appear in 'test-room' projection"
                );
            }
        }
    }

    // Deregister callback before freeing: prevents the lingering listener
    // thread from calling into a context pointer after this frame unwinds.
    test_app_set_update_callback(app, std::ptr::null_mut(), None);
    test_app_free(app);
}
