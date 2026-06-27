//! Registry-coverage gate: every projection key the `nmp-codegen` Swift
//! registry promises to decode must be a key the kernel actually produces at
//! runtime.
//!
//! ## The defect class this closes (#1084 pattern, ADR-0048 signer_state)
//!
//! The sibling gate (`every_generic_projection_key_has_a_typed_sidecar`)
//! protects the producer's OWN keyspace: generic key ⇒ typed twin. It cannot
//! see a *consumer-side* registry drifting from the producer: when the kernel
//! renamed `bunker_connection_state` → `signer_state` (ADR-0048 D6), the
//! codegen registry — and therefore the generated Swift decoders, the iOS
//! bridge, and the Android `@SerialName` — kept decoding the OLD key. Every
//! existing CI gate compares artifacts that all used the old key consistently,
//! so nothing failed while the #1098 bunker badge went permanently dark on
//! both platforms.
//!
//! This gate puts the codegen registry and the runtime producer in ONE
//! assertion: each registry `key` must be either
//! - a key with a registered Tier-1 closure in the live `SnapshotRegistry`
//!   (host registrations + the actor's built-ins — they share one
//!   `Arc<Mutex<…>>`), or
//! - a member of [`nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS`] (the Tier-2
//!   built-ins `make_update` inserts directly; pinned against the insertion
//!   code by `nmp-core`'s `builtin_projection_keys_const_matches_runtime`).
//!
//! A producer-side key rename that ships without updating the codegen
//! registry — or a registry entry added without its producer — fails HERE.
//!
//! ## Scope
//!
//! Entries with `typed_sidecar: Some(..)` are gated: those are the keys hosts
//! actually decode from the wire (the generic `payload:Value` slot was deleted
//! in PR-B #1082, so a sidecar-less entry has no wire form at all today — the
//! remaining `None` entries, `last_action_result` / `timeline` / `inserted` /
//! `updated` / `removed`, are vestigial Swift-struct fields from the JSON era
//! and are exempt rather than asserted-on; deleting them from the registry is
//! tracked separately).
//!
//! `nmp.marmot.*` is exempt unless this crate's `marmot` feature is enabled:
//! the Marmot registration (and its 5,400-LOC MLS dependency tree) is
//! feature-gated out of the default test build, so the producer cannot be
//! observed here; the Marmot crate's own tests own that proof.
//!
//! ## Synchronisation (no polling)
//!
//! The actor registers its Tier-1 built-ins (`bunker_handshake` /
//! `nip46_onboarding` / `signer_state`) on the actor thread between receiving
//! the FIRST command and dispatching it. The test therefore installs an
//! update callback, sends `Start`, and block-recvs frames until one decodes
//! with `running == true` — that frame is emitted strictly AFTER the
//! registration block ran, so the registry read below is race-free. Each
//! `recv_timeout` is a blocking wait on a real channel, not a sleep loop.

use std::collections::BTreeSet;
use std::ffi::{CString, c_void};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use nmp_codegen::swift_projections_registry::SNAPSHOT_PROJECTIONS;
use nmp_core::decode_snapshot_envelope;
use nmp_ffi::{NmpApp, nmp_app_free, nmp_app_new, nmp_app_set_update_callback, nmp_app_start};

use super::super::super::{
    nmp_app_chirp_close_group_discovery, nmp_app_chirp_open_group_discovery,
    nmp_app_chirp_register, nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list,
    nmp_app_chirp_register_group_events, nmp_app_chirp_unregister,
};

#[cfg(feature = "marmot")]
use super::super::helpers::register_marmot_for_test;

/// `extern "C"` callbacks cannot capture; park the frame `Sender` in a static
/// and forward every emitted frame's bytes through it (the V-82 test pattern).
static FRAME_TX: OnceLock<Mutex<Option<Sender<Vec<u8>>>>> = OnceLock::new();

extern "C" fn forward_frame(_ctx: *mut c_void, bytes: *const u8, len: usize) {
    if bytes.is_null() {
        return;
    }
    // SAFETY: the FFI listener owns `bytes` for the duration of this call.
    let frame = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
    if let Some(slot) = FRAME_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(frame);
            }
        }
    }
}

/// THE GATE: bootstrap the full Chirp surface, wait (event-driven) for the
/// actor's registration block to have run, then assert every codegen-registry
/// key with a typed sidecar is a key the kernel produces at runtime.
#[test]
fn every_codegen_registry_key_is_registered_at_runtime() {
    let app = nmp_app_new();
    assert!(!app.is_null());

    // Install the frame forwarder BEFORE Start so the running=true frame
    // cannot be missed.
    let (tx, rx) = channel::<Vec<u8>>();
    *FRAME_TX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("frame tx slot") = Some(tx);

    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(forward_frame));
    nmp_app_start(app, 64, 8);

    // Block until a frame from the REAL kernel arrives (`running == true`).
    // The pre-flight frame (`running == false`) may or may not be observed
    // first depending on callback-installation timing; skip it. The actor
    // registers `signer_state` et al. BEFORE dispatching `Start`, so a
    // running frame proves the registration block completed.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("actor never emitted a running=true frame within 10s");
        let frame = rx
            .recv_timeout(remaining)
            .expect("update-frame channel closed or timed out before running=true");
        if matches!(decode_snapshot_envelope(&frame), Ok(env) if env.running) {
            break;
        }
    }

    // Bootstrap the full Chirp registration surface (same as the sibling
    // gate). These register synchronously on the caller thread through the
    // shared registry Arc.
    let viewer = CString::new("aa".repeat(32)).unwrap();
    let mut handle = std::ptr::null_mut();
    let status = nmp_app_chirp_register(app, viewer.as_ptr(), &mut handle);
    assert_eq!(status, super::super::super::NmpRegisterStatus::Ok as u32);
    assert!(!handle.is_null());
    nmp_app_chirp_register_dm_inbox(app);
    let active = CString::new("aa".repeat(32)).unwrap();
    nmp_app_chirp_register_follow_list(app, active.as_ptr());
    // open_group_discovery returns a handle that must be closed before nmp_app_free.
    let host = CString::new("wss://groups.example.com").unwrap();
    let discovery_handle = nmp_app_chirp_open_group_discovery(app, host.as_ptr());
    // NOTE: the request shape is `{group:{host_relay_url, local_id}, kinds:[…]}`
    // (issue #2187). A body missing `kinds` (or with a wrong group shape) fails
    // deserialization and silently no-ops the registration (D6). This gate
    // guards that the `nmp.nip29.group_events` key actually lands in the runtime
    // keyset, not just the codegen registry.
    let group_request = CString::new(
        r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"abcd"},"kinds":[9,11]}"#,
    )
    .unwrap();
    nmp_app_chirp_register_group_events(app, group_request.as_ptr());

    #[cfg(feature = "marmot")]
    let marmot = register_marmot_for_test(app, "registry-coverage");

    // Runtime keyset: Tier-1 typed sidecar closures ∪ the Tier-2 kernel built-ins.
    // Use `registered_typed_projection_keys()` (returns registered keys WITHOUT
    // calling closures) rather than `run_typed_snapshot_projections()` (returns
    // only keys whose closures emit Some for the current state). Projections for
    // optional features (e.g. `bunker_handshake` with no active NIP-46 session)
    // would be absent from the run output but are present in the registry.
    let app_ref: &NmpApp = unsafe { &*app };
    let mut runtime_keys: BTreeSet<String> = app_ref
        .registered_typed_projection_keys()
        .into_iter()
        .collect();
    runtime_keys.extend(
        nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS
            .iter()
            .map(|k| (*k).to_string()),
    );

    // Feature-gated exemptions — keys whose producer is compiled out of this
    // test build. Each entry must name the gating feature.
    let mut exempt: BTreeSet<&str> = BTreeSet::new();
    if cfg!(not(feature = "marmot")) {
        exempt.insert("nmp.marmot.snapshot"); // feature = "marmot"
        exempt.insert("nmp.marmot.messages"); // feature = "marmot"
    }
    if cfg!(not(feature = "wallet")) {
        exempt.insert("wallet"); // feature = "wallet" (default-on)
    }

    let uncovered: Vec<&str> = SNAPSHOT_PROJECTIONS
        .iter()
        .filter(|entry| entry.typed_sidecar.is_some())
        .map(|entry| entry.key)
        .filter(|key| !exempt.contains(key) && !runtime_keys.contains(*key))
        .collect();
    assert!(
        uncovered.is_empty(),
        "codegen-registry projection keys with NO runtime producer — the \
         generated Swift/Kotlin decoders read these keys, but the kernel never \
         emits them, so the corresponding UI surface is permanently dark \
         (the ADR-0048 bunker-badge failure mode). Either the producer renamed \
         its key without updating crates/nmp-codegen/src/swift_projections_registry.rs \
         (+ regenerating the bridges), or a registry entry was added without \
         its producer:\n  uncovered: {uncovered:?}\n  runtime keys: {runtime_keys:?}"
    );

    // Guard against a vacuous pass: the registry must contribute a
    // non-trivial gated keyspace and the runtime must report one.
    assert!(
        SNAPSHOT_PROJECTIONS
            .iter()
            .filter(|e| e.typed_sidecar.is_some())
            .count()
            >= 20,
        "the codegen registry lost most of its typed entries — the gate is \
         no longer exercising the real consumer surface"
    );
    assert!(
        runtime_keys.len() >= 10,
        "runtime reported only {} projection keys ({runtime_keys:?}) — the \
         bootstrap is not exercising the real registration surface",
        runtime_keys.len()
    );

    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    if let Some(slot) = FRAME_TX.get() {
        *slot.lock().expect("frame tx slot") = None;
    }
    if !discovery_handle.is_null() {
        nmp_app_chirp_close_group_discovery(discovery_handle);
    }
    #[cfg(feature = "marmot")]
    drop(marmot);
    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
