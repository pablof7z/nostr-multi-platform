//! Shared harness for the NIP-29 typed-projection sidecar proof tests
//! (`typed_group_chat_sidecar.rs`, `typed_discovered_groups_sidecar.rs`).
//!
//! Drives the full FFI snapshot path — boot an `NmpApp`, run the actor, collect
//! every emitted `UpdateFrame`, decode each with `decode_snapshot_typed_projections`,
//! and surface the typed sidecar entry for a key once a predicate accepts it.
//! This is the same path any host shell reads from; the projections are wired by
//! `nmp_nip29::register::{wire_group_chat, open_group_discovery}`, which emit
//! a typed FlatBuffers sidecar (ADR-0037) alongside the generic `Value` tree.
//!
//! Split out of a single test file so each test file stays under the AGENTS.md
//! 300-LoC ceiling.

use std::ffi::c_void;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use nmp_store::{RawEvent, VerifiedEvent};
use nmp_core::{decode_snapshot_typed_projections, ActorCommand, TypedProjectionData};
use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_app_set_update_callback, nmp_app_start, NmpApp};

/// NmpApp instances spawn global actor threads that do not cleanly isolate
/// across parallel test processes — every test in either file serialises on
/// this lock.
pub static SERIAL: Mutex<()> = Mutex::new(());

/// Raw frame bytes collected by the update callback (one entry per tick).
static FRAMES: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

/// The host relay every test wires its projections to.
pub const HOST: &str = "wss://groups.example.com";

extern "C" fn collect_frame(_ctx: *mut c_void, bytes: *const u8, len: usize) {
    if bytes.is_null() {
        return;
    }
    // SAFETY: the FFI listener owns `bytes` for the duration of this call.
    let frame = unsafe { std::slice::from_raw_parts(bytes, len) };
    FRAMES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(frame.to_vec());
}

/// Build a `RawEvent`. `id` / `author` must be 64-hex (as real Nostr ids /
/// pubkeys always are): empirically, addressable 39xxx events injected with a
/// non-hex id/pubkey never surface to the projection — they are dropped on the
/// `IngestPreVerifiedEvents` store path before observers are notified, whereas
/// the kind:9 group-chat path accepts them. Use hex coordinates throughout.
pub fn raw_event(
    id: &str,
    author: &str,
    kind: u32,
    ts: u64,
    tags: Vec<Vec<String>>,
    content: &str,
) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at: ts,
        kind,
        tags,
        content: content.to_string(),
        sig: "0".repeat(128),
    }
}

/// Inject pre-verified events via the actor channel — the exact path a relay
/// worker takes after signature verification.
pub fn inject(app: *mut NmpApp, events: Vec<VerifiedEvent>) {
    // SAFETY: `app` is a valid pointer from `nmp_app_new` owned by the caller.
    let app_ref = unsafe { &*app };
    app_ref
        .actor_sender()
        .send(ActorCommand::IngestPreVerifiedEvents(events))
        .expect("actor command channel must be open");
}

/// Poll collected frames, decoding each with `decode_snapshot_typed_projections`, until
/// a tick carries a typed sidecar entry under `key` that `predicate` accepts (or
/// the 3-second deadline passes). Returns the matching typed entry.
pub fn wait_for_typed(
    key: &str,
    predicate: impl Fn(&TypedProjectionData) -> bool,
) -> Option<TypedProjectionData> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        {
            let frames = FRAMES.lock().unwrap_or_else(|p| p.into_inner());
            for frame in frames.iter() {
                let Ok(typed) = decode_snapshot_typed_projections(frame) else {
                    continue;
                };
                if let Some(entry) = typed.into_iter().find(|t| t.key == key && predicate(t)) {
                    return Some(entry);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// Boot an app with the frame-collecting callback and the actor running.
/// Clears the shared frame buffer first (tests are serialised via [`SERIAL`]).
pub fn boot() -> *mut NmpApp {
    FRAMES.lock().unwrap_or_else(|p| p.into_inner()).clear();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(collect_frame));
    nmp_app_start(app, 0, 64, 8); // emit_hz=8 → ~125 ms cadence
    app
}

/// Deregister the callback before freeing so the listener thread never calls
/// into a freed context pointer.
pub fn teardown(app: *mut NmpApp) {
    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);
}
