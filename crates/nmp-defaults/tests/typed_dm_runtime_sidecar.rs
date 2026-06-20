//! NIP-17 DM-runtime typed-projection sidecar proof (Wave A producer-typing).
//!
//! Proves `nmp_defaults::runtimes::register_dm_runtime` emits typed
//! FlatBuffers sidecars (ADR-0037) for BOTH DM keys:
//!
//! - `"nmp.nip17.dm_inbox"` (`NDMI`) — `nmp_nip17::DmInboxSnapshot`.
//! - `"nmp.nip17.dm_relay_list"` (`NDRL`) — `nmp_nip17::DmRelayList`.
//!
//! Drives the full FFI snapshot path (boot an `NmpApp`, run the actor, collect
//! every emitted frame, decode each with `decode_snapshot_typed_projections`), then
//! asserts the typed payload bytes land in the `typed_projections` sidecar and
//! round-trip back through the generated bindings. No events are injected — both
//! projections emit a decodable buffer every tick (empty inbox / not-signed-in
//! relay list), which is exactly what proves the registration wiring fires.

use std::ffi::c_void;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use nmp_core::{decode_snapshot_typed_projections, TypedProjectionData};
use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_app_set_update_callback, nmp_app_start, NmpApp};
use nmp_nip17::{
    decode_dm_inbox_snapshot, decode_dm_relay_list, DM_INBOX_FILE_IDENTIFIER, DM_INBOX_SCHEMA_ID,
    DM_RELAY_LIST_FILE_IDENTIFIER, DM_RELAY_LIST_SCHEMA_ID,
};

/// NmpApp instances spawn global actor threads; serialise every test on it.
static SERIAL: Mutex<()> = Mutex::new(());
/// Raw frame bytes collected by the update callback (one entry per tick).
static FRAMES: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

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

fn boot() -> *mut NmpApp {
    FRAMES.lock().unwrap_or_else(|p| p.into_inner()).clear();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(collect_frame));
    nmp_app_start(app, 64, 8); // emit_hz=8 → ~125 ms cadence
    app
}

fn teardown(app: *mut NmpApp) {
    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);
}

/// Poll collected frames until a tick carries a typed sidecar entry under `key`
/// that `predicate` accepts (or the 3-second deadline passes).
fn wait_for_typed(
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

/// `register_dm_runtime` wires the `NDMI` typed sidecar for `nmp.nip17.dm_inbox`
/// alongside the generic `Value` projection; the payload decodes back into a
/// `DmInboxSnapshot` (empty, not signed in).
#[test]
fn dm_inbox_typed_sidecar_surfaces() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for this block.
    nmp_defaults::runtimes::register_dm_runtime(unsafe { &*app });

    let entry = wait_for_typed("nmp.nip17.dm_inbox", |t| {
        decode_dm_inbox_snapshot(&t.payload).is_ok()
    })
    .expect("dm_inbox typed sidecar must appear within 3 s");

    assert_eq!(entry.schema_id, DM_INBOX_SCHEMA_ID);
    assert_eq!(
        entry.file_identifier,
        String::from_utf8_lossy(DM_INBOX_FILE_IDENTIFIER)
    );
    assert!(!entry.payload.is_empty(), "typed payload must carry bytes");

    let snapshot = decode_dm_inbox_snapshot(&entry.payload)
        .expect("NDMI payload must decode back into DmInboxSnapshot");
    assert!(
        snapshot.conversations.is_empty(),
        "no events injected → empty inbox, got {:?}",
        snapshot.conversations
    );

    teardown(app);
}

/// `register_dm_runtime` also wires the `NDRL` typed sidecar for
/// `nmp.nip17.dm_relay_list`; the payload decodes back into a `DmRelayList`
/// (no active pubkey, no relays — not signed in).
#[test]
fn dm_relay_list_typed_sidecar_surfaces() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    // SAFETY: valid pointer from `nmp_app_new`, live for this block.
    nmp_defaults::runtimes::register_dm_runtime(unsafe { &*app });

    let entry = wait_for_typed("nmp.nip17.dm_relay_list", |t| {
        decode_dm_relay_list(&t.payload).is_ok()
    })
    .expect("dm_relay_list typed sidecar must appear within 3 s");

    assert_eq!(entry.schema_id, DM_RELAY_LIST_SCHEMA_ID);
    assert_eq!(
        entry.file_identifier,
        String::from_utf8_lossy(DM_RELAY_LIST_FILE_IDENTIFIER)
    );
    assert!(!entry.payload.is_empty(), "typed payload must carry bytes");

    let relay_list = decode_dm_relay_list(&entry.payload)
        .expect("NDRL payload must decode back into DmRelayList");
    assert_eq!(
        relay_list.active_pubkey, None,
        "not signed in → active_pubkey None"
    );
    assert!(
        relay_list.read_relay_urls.is_empty(),
        "no relays configured → empty list, got {:?}",
        relay_list.read_relay_urls
    );

    teardown(app);
}
