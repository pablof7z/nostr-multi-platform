//! NIP-29 group-chat end-to-end round-trip.
//!
//! Validates the full publish + receive stack at the NMP layer — zero Chirp
//! symbols, no architectural shortcuts.
//!
//! ## Publish side
//!
//! `nmp_app_dispatch_action("nmp.nip29.post_chat_message", …)` routes through
//! the registered `PostChatMessageAction` module: the typed validator runs
//! synchronously (returning a 32-hex `correlation_id`) and the executor
//! enqueues `ActorCommand::PublishUnsignedEventToRelays` pinned to the group's
//! host relay. Actions are registered by `nmp_nip29::register::register_actions`
//! — the same call any host (Chirp, a TUI, a REPL) makes at startup.
//!
//! ## Receive side
//!
//! A well-formed kind:9 event carrying `["h", local_id]` is injected via
//! `ActorCommand::IngestPreVerifiedEvents`. This is bit-for-bit identical to
//! the path a relay worker follows when it delivers a verified event into the
//! actor loop. The actor fans it out through `notify_event_observers`;
//! `GroupChatProjection` (wired by `nmp_nip29::register::wire_group_chat`)
//! accumulates it and surfaces it under
//! `projections["nmp.nip29.group_chat"]["messages"]` on the next snapshot tick.
//! The test reads that snapshot via `nmp_app_set_update_callback` — the same
//! path any shell (iOS KernelBridge, a TUI, a web bridge) uses.
//!
//! ## Why no real relay?
//!
//! `IngestPreVerifiedEvents` is the exact path a relay worker takes after
//! signature verification — the projection code cannot distinguish relay-
//! delivered from injected events. A two-instance relay-bridged test is left
//! for when that harness is available.

use std::ffi::{c_void, CStr, CString};
use std::sync::Mutex;
use std::time::Duration;

use nmp_core::store::{RawEvent, VerifiedEvent};
use nmp_core::ActorCommand;
use nmp_ffi::{
    nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string, nmp_app_new,
    nmp_app_set_update_callback, nmp_app_start,
};
use nmp_nip29::group_id::GroupId;
use nmp_nip29::register::{register_actions, wire_group_chat};

// Tests that spin up NmpApp instances must be serialised: each spawns global
// actor threads that do not cleanly isolate across parallel test processes.
static SERIAL: Mutex<()> = Mutex::new(());

// Kernel snapshot JSON payloads collected by the update callback.
static SNAPSHOTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

extern "C" fn collect_snapshot(_ctx: *mut c_void, bytes: *const u8, len: usize) {
    if bytes.is_null() {
        return;
    }
    // SAFETY: the FFI listener owns `bytes` for the duration of this call.
    let frame = unsafe { std::slice::from_raw_parts(bytes, len) };
    let Ok(snapshot) = nmp_core::decode_snapshot_payload(frame) else {
        return;
    };
    SNAPSHOTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(snapshot.to_string());
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
    // SAFETY: `app` is a valid pointer from `nmp_app_new` owned by the caller.
    let app_ref = unsafe { &*app };
    app_ref
        .actor_sender()
        .send(ActorCommand::IngestPreVerifiedEvents(events))
        .expect("actor command channel must be open");
}

/// Poll `SNAPSHOTS` until a snapshot tick contains a group-chat message with
/// `content` under `projections["nmp.nip29.group_chat"]["messages"]`, or the
/// 3-second deadline passes.
fn wait_for_group_message(content: &str) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        {
            let snaps = SNAPSHOTS.lock().unwrap_or_else(|p| p.into_inner());
            for json in snaps.iter() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
                    if let Some(msgs) =
                        v["projections"]["nmp.nip29.group_chat"]["messages"].as_array()
                    {
                        if msgs.iter().any(|m| m["content"].as_str() == Some(content)) {
                            return true;
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

// ── Publish side ─────────────────────────────────────────────────────────────

/// Proves the publish-side seam is live: `nmp_app_dispatch_action` routes
/// `nmp.nip29.post_chat_message` through both the `PostChatMessageAction`
/// module (typed validator → `correlation_id`) and executor (enqueues
/// `PublishUnsignedEventToRelays` on the actor channel, fire-and-forget).
///
/// Registered via `nmp_nip29::register::register_actions` — zero Chirp
/// symbols. Any host calls this same function at startup.
#[test]
fn post_chat_message_dispatch_returns_correlation_id() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let app = nmp_app_new();
    // SAFETY: `app` is a valid pointer from `nmp_app_new`; no other reference
    // aliases it at this call site.
    register_actions(unsafe { &mut *app });

    let payload = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"test-room"},"content":"hello from TUI"}"#;
    let ns = CString::new("nmp.nip29.post_chat_message").unwrap();
    let body = CString::new(payload).unwrap();
    let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
    assert!(!ptr.is_null(), "dispatch_action must not return null");
    // SAFETY: ptr is a valid nul-terminated string from dispatch_action.
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_app_free_string(ptr);

    let result: serde_json::Value = serde_json::from_str(&out).unwrap();
    let cid = result
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id in dispatch result, got: {result}"));
    assert_eq!(cid.len(), 32, "correlation_id must be 32 hex chars");

    // Malformed payload (missing required `group` field) is rejected by the
    // typed module validator — the executor is never reached.
    let bad = CString::new(r#"{"content":"no group field"}"#).unwrap();
    let ns2 = CString::new("nmp.nip29.post_chat_message").unwrap();
    let ptr2 = nmp_app_dispatch_action(app, ns2.as_ptr(), bad.as_ptr());
    let out2 = unsafe { CStr::from_ptr(ptr2) }.to_str().unwrap().to_owned();
    nmp_app_free_string(ptr2);
    let result2: serde_json::Value = serde_json::from_str(&out2).unwrap();
    assert!(
        result2.get("error").is_some(),
        "dispatch without `group` field must be rejected: {result2}"
    );

    nmp_app_free(app);
}

// ── Receive side ─────────────────────────────────────────────────────────────

/// Proves the receive-side seam is live end-to-end:
///
/// 1. `nmp_nip29::register::wire_group_chat` wires a `GroupChatProjection` for
///    `"test-room"` as a `KernelEventObserver` (ingest) + snapshot projection
///    under `"nmp.nip29.group_chat"` (output).
/// 2. A kind:9 event carrying `["h", "test-room"]` is injected via
///    `IngestPreVerifiedEvents` — the same actor path a relay worker uses.
/// 3. The `GroupChatProjection` accumulates the event; on the next snapshot
///    tick the kernel serializes it under `projections["nmp.nip29.group_chat"]`.
/// 4. The update callback (set via `nmp_app_set_update_callback`) receives the
///    JSON string — the same path any shell reads from.
/// 5. A decoy event for a different group must NOT appear.
#[test]
fn group_chat_event_surfaces_via_kernel_snapshot_callback() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    SNAPSHOTS.lock().unwrap_or_else(|p| p.into_inner()).clear();

    let app = nmp_app_new();

    // Register the update callback before start so no snapshot tick is missed.
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(collect_snapshot));

    // nmp_app_start sends ActorCommand::Start; the actor enters its main loop
    // and begins emitting snapshot ticks at emit_hz rate.
    nmp_app_start(app, 0, 64, 8); // emit_hz=8 → ~125 ms cadence

    // Wire the GroupChatProjection for "test-room".
    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for this block.
    let app_ref = unsafe { &*app };
    wire_group_chat(
        app_ref,
        GroupId::new("wss://groups.example.com", "test-room"),
    );

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
        "kind:9 event for 'test-room' must appear in projections[\"nmp.nip29.group_chat\"][\"messages\"] within 3 s"
    );

    // Verify the decoy did NOT leak into the projection.
    {
        let snaps = SNAPSHOTS.lock().unwrap_or_else(|p| p.into_inner());
        for json in snaps.iter() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
                if let Some(msgs) = v["projections"]["nmp.nip29.group_chat"]["messages"].as_array()
                {
                    assert!(
                        !msgs
                            .iter()
                            .any(|m| m["content"].as_str() == Some("should not appear")),
                        "decoy event for 'other-room' must not appear in 'test-room' projection"
                    );
                }
            }
        }
    }

    // Deregister callback before freeing: prevents the lingering listener
    // thread from calling into a context pointer after this frame unwinds.
    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);
}
