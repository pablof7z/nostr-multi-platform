mod actor;
mod app;
pub mod bunker_hook;
mod capability_socket;
mod ffi;
mod ffi_guard;
mod keepalive;
mod kernel;
mod kernel_reducer;
pub mod nip19;
pub mod nip21;
pub mod planner;
pub mod publish;
mod relay;
mod relay_worker;
pub mod remote_signer;
pub mod store;
pub mod subs;
pub mod substrate;
pub mod tags;
mod update_envelope;
pub mod util;

pub use app::{
    resolve_open_uri, KernelAction, KernelUpdate, KernelViewSpec, OpenUriError, OpenUriRouting,
    VIEW_ADDRESSABLE, VIEW_PROFILE, VIEW_THREAD,
};
pub use kernel_reducer::KernelReducer;
pub use bunker_hook::{register_bunker_hook, BunkerHookFn, BunkerHookRequest};
pub use ffi::NmpApp;
pub use remote_signer::RemoteSignerHandle;
pub use update_envelope::{
    panic_message, wrap_panic, wrap_snapshot, wrap_update, DeltaEnvelope, PanicFrame,
    UpdateEnvelope, WireDelta, WireEnvelope, DELTA_SCHEMA_VERSION, SNAPSHOT_SCHEMA_VERSION,
};

// Stage 4 of NIP-46 wiring: `nmp-signer-broker` (the crate that bridges
// `nmp-core` and `nmp-signers`) needs to construct `ActorCommand` values to
// push `AddRemoteSigner` / `BunkerHandshakeProgress` back to the actor. The
// `actor` module is crate-private so this re-export is the only path. The
// enum variants themselves are already `pub`.
pub use actor::ActorCommand;

// Re-export the FFI entry-points so the ffi-stress harness (and any other
// Rust-side crate) can call them directly via the Rust rlib dependency,
// without an `extern "C"` block. The symbols remain `#[no_mangle]` on the
// ffi:: side and are still reachable from Swift/C unchanged.
#[cfg(any(test, feature = "test-support"))]
pub use ffi::{
    nmp_app_cancel_publish, nmp_app_claim_profile, nmp_app_close_author, nmp_app_close_thread,
    nmp_app_configure, nmp_app_dispatch_action, nmp_app_dispatch_capability, nmp_app_free,
    nmp_app_free_string, nmp_app_inject_pre_verified_events, nmp_app_inject_signed_events,
    nmp_app_lifecycle_background, nmp_app_lifecycle_foreground, nmp_app_new, nmp_app_open_author,
    nmp_app_open_firehose_tag, nmp_app_open_thread, nmp_app_open_uri,
    nmp_app_publish_signed_event, nmp_app_publish_signed_event_to,
    nmp_app_publish_unsigned_event, nmp_app_register_event_observer,
    nmp_app_register_raw_event_observer, nmp_app_release_profile, nmp_app_retry_publish,
    nmp_app_set_capability_callback, nmp_app_set_lifecycle_callback, nmp_app_set_storage_path,
    nmp_app_set_update_callback, nmp_app_signin_nsec, nmp_app_start,
    nmp_app_unregister_event_observer, nmp_app_unregister_raw_event_observer,
};

// android-ffi: expose the full FFI surface via Rust paths. nmp-android-ffi
// calls these through the rlib dependency — this is what causes rustc to
// include the symbol bodies in CGU files. Without Rust-path references the
// rlib is consumed at compile time but the symbols stay `U` in the cdylib.
#[cfg(feature = "android-ffi")]
pub use ffi::{
    nmp_app_add_relay,
    nmp_app_cancel_publish,
    nmp_app_claim_profile,
    nmp_app_close_author,
    nmp_app_close_thread,
    nmp_app_configure,
    nmp_app_create_new_account,
    nmp_app_dispatch_action,
    nmp_app_dispatch_capability,
    nmp_app_follow,
    nmp_app_free,
    nmp_app_free_string,
    // T118 / G3 — lifecycle symbols must be reachable from the Android JNI
    // shim too; same rationale as every other entry in this block.
    nmp_app_lifecycle_background,
    nmp_app_lifecycle_foreground,
    nmp_app_new,
    nmp_app_open_author,
    nmp_app_open_firehose_tag,
    nmp_app_open_thread,
    nmp_app_open_timeline,
    nmp_app_open_uri,
    nmp_app_publish_signed_event,
    nmp_app_publish_signed_event_to,
    nmp_app_publish_unsigned_event,
    nmp_app_react,
    // T146 — kernel event observer FFI for Android JNI.
    nmp_app_register_event_observer,
    // Raw signed-event tap FFI for Android JNI.
    nmp_app_register_raw_event_observer,
    nmp_app_release_profile,
    nmp_app_remove_account,
    nmp_app_remove_relay,
    nmp_app_retry_publish,
    nmp_app_set_capability_callback,
    nmp_app_set_lifecycle_callback,
    nmp_app_set_storage_path,
    nmp_app_set_update_callback,
    nmp_app_signin_bunker,
    nmp_app_signin_nsec,
    nmp_app_start,
    nmp_app_stop,
    nmp_app_switch_active,
    nmp_app_unfollow,
    nmp_app_unregister_event_observer,
    nmp_app_unregister_raw_event_observer,
};

// D0: NIP-47 NWC is an app noun — the `nmp_app_wallet_*` FFI symbols are
// gated behind the `wallet` Cargo feature. Re-exported via Rust paths for
// the Android JNI shim only when both features are on.
#[cfg(all(feature = "android-ffi", feature = "wallet"))]
pub use ffi::{nmp_app_wallet_connect, nmp_app_wallet_disconnect, nmp_app_wallet_pay_invoice};

// T118 / G3 — lifecycle observer wire-shape exposed for integration tests
// (the `LifecycleObserverFn` is a plain `extern "C" fn` shape) and the
// phase-code constants the observer must interpret. The actor module is
// crate-private, so this is the only Rust-side surface for the wire shape.
#[cfg(any(test, feature = "test-support"))]
pub use actor::{LifecycleObserverFn, LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};

// T146 — kernel event observer surface exposed to per-app Rust crates
// (`nmp-app-chirp`, future app-specific crates, ...). Apps register typed
// `Arc<dyn KernelEventObserver>`s via [`NmpApp::register_event_observer`].
// The FFI shape (`KernelEventObserverFn` etc.) is the C-ABI channel
// Swift / Kotlin bridges use directly through
// `nmp_app_register_event_observer`.
pub use actor::{
    KernelEventObserver, KernelEventObserverFn, KernelEventObserverId,
    KernelEventObserverRegistration,
};

// Raw signed-event tap surface exposed to per-app Rust crates. Apps
// register typed `Arc<dyn RawEventObserver>`s (with a `KindFilter`) via
// [`NmpApp::register_raw_event_observer`] to receive the verbatim flat
// NIP-01 signed event (`sig` included). The FFI shape
// (`RawEventObserverFn` etc.) is the C-ABI channel Swift / Kotlin bridges
// use directly through `nmp_app_register_raw_event_observer`. Generic
// capability (D0) — no protocol nouns.
pub use actor::{
    KindFilter, RawEventObserver, RawEventObserverFn, RawEventObserverId,
    RawEventObserverRegistration,
};

/// Test-support facade: gives live-bench binaries access to the actor
/// internals without exposing domain nouns in the stable `nmp-core` API.
///
/// Enable with `features = ["test-support"]` in `Cargo.toml`.  This gate is
/// intentionally `any(test, feature = "test-support")` so `cargo test` always
/// has access without an explicit feature flag.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    pub use crate::actor::{run_actor, ActorCommand};
    pub use crate::store::{RawEvent, VerifiedEvent};

    /// NIP golden-tag conformance harness — drives the (crate-private) command
    /// handlers against a real `Kernel` + `IdentityRuntime` and returns the
    /// emitted `EVENT` JSON so an integration test can assert per-kind tag
    /// structure. See `tests/nip_tag_conformance.rs`.
    pub use crate::actor::ConformanceHarness;

    use std::sync::mpsc;
    use std::thread;

    /// Spawn the kernel actor on a dedicated thread.
    ///
    /// Returns a command sender and an update receiver.  The caller drives the
    /// actor by sending [`ActorCommand`] values and reads JSON-encoded kernel
    /// snapshots from the update channel.  Dropping the sender or sending
    /// [`ActorCommand::Shutdown`] stops the actor thread.
    pub fn spawn_actor() -> (mpsc::Sender<ActorCommand>, mpsc::Receiver<String>) {
        let (command_tx, command_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        thread::spawn(move || run_actor(command_rx, update_tx));
        (command_tx, update_rx)
    }

    /// Build `count` real Schnorr-signed kind-1 events and enqueue them for
    /// ingest via `ActorCommand::IngestPreVerifiedEvents`.
    ///
    /// Uses a single `nostr::Keys::generate()` fixture key so all events share
    /// one pubkey — sufficient for harness pressure tests (S4/S5) where the
    /// goal is emit throughput, not per-author diversity.
    ///
    /// Schnorr sign cost: ~30–50 µs/event.  For S4 (500 events) and S5 (200
    /// events) this is 10–25 ms total — acceptable.  For S3 (100k events) use
    /// `nmp_app_inject_pre_verified_events` which uses `from_raw_unchecked`.
    #[allow(clippy::result_large_err)] // ActorCommand is large by design; boxing here would cascade through test callers
    pub fn inject_signed_events(
        tx: &mpsc::Sender<ActorCommand>,
        base_ts: u64,
        count: u32,
    ) -> Result<(), mpsc::SendError<ActorCommand>> {
        use nostr::{EventBuilder, Keys, Timestamp};

        // Single fixture key: generate once, sign all events with it.
        // The key is not reused across harness runs (Keys::generate() uses OsRng).
        let keys = Keys::generate();
        let events: Vec<VerifiedEvent> = (0..count as u64)
            .filter_map(|i| {
                let content = format!("signed harness event {i}");
                let ts = Timestamp::from(base_ts.saturating_add(i));
                let nostr_event = EventBuilder::text_note(content)
                    .custom_created_at(ts)
                    .sign_with_keys(&keys)
                    .ok()?;
                // Convert nostr::Event to our RawEvent, then verify the full path.
                // try_from_raw re-verifies the signature — confirms the signed event
                // is well-formed before the kernel ingests it.
                let raw = RawEvent {
                    id: nostr_event.id.to_hex(),
                    pubkey: nostr_event.pubkey.to_hex(),
                    created_at: nostr_event.created_at.as_secs(),
                    kind: nostr_event.kind.as_u16() as u32,
                    tags: nostr_event
                        .tags
                        .iter()
                        .map(|t| t.as_slice().to_vec())
                        .collect(),
                    content: nostr_event.content.clone(),
                    sig: nostr_event.sig.to_string(),
                };
                VerifiedEvent::try_from_raw(raw).ok()
            })
            .collect();
        tx.send(ActorCommand::IngestPreVerifiedEvents(events))
    }
}
