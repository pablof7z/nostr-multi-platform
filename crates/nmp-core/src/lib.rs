mod actor;
mod app;
pub mod bunker_hook;
// V6 Stage 1 — Swift `Decodable` emitter input surface. Feature-gated:
// `cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas`
// dumps one JSON schema per pilot projection type for `nmp-codegen gen swift`
// to consume. Off by default — shipped artifacts never link `schemars`.
#[cfg(feature = "codegen-schema")]
pub mod codegen_schema;
mod capability_socket;
// ffi: C-ABI entry points for Swift/Kotlin native shells.
// Gated on `native` — wasm32 uses wasm-bindgen, not C-ABI.
#[cfg(feature = "native")]
mod ffi;
// ffi_guard: pure catch_unwind wrapper. Not I/O-bound; kept always-on
// because actor/commands/* use it on the native side (also actor is always
// compiled until Phase 1c decoupling). If actor is gated in a future PR,
// ffi_guard can be folded into the native gate alongside it.
mod ffi_guard;
mod keepalive;
mod kernel;
mod kernel_action;
mod kernel_reducer;
pub mod nip19;
pub mod nip21;
pub mod planner;
pub mod publish;
mod relay;
// V-01 Phase 1c: the WebSocket relay worker is the native I/O layer.
// Gated behind `native` (matches the `tungstenite`/`mio`/`rustls` dep gate
// in Cargo.toml). The kernel speaks [`crate::kernel::RelayFrame`] instead of
// `tungstenite::Message` so it compiles without this module.
#[cfg(feature = "native")]
mod relay_worker;
pub mod remote_signer;
pub mod stable_hash;
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
pub use bunker_hook::{register_bunker_hook, BunkerHookFn, BunkerHookRequest};
#[cfg(feature = "native")]
pub use ffi::NmpApp;
pub use kernel::{read_eligible_relay_urls, RelayEditRow, RelayEditRowList, RelayEditRowsSlot};
pub use kernel_reducer::KernelReducer;
pub use relay::canonical_relay_url;
pub use remote_signer::RemoteSignerHandle;
pub use update_envelope::{
    panic_message, wrap_panic, wrap_snapshot, PanicFrame, UpdateEnvelope, WireEnvelope,
    SNAPSHOT_SCHEMA_VERSION,
};

// Stage 4 of NIP-46 wiring: `nmp-signer-broker` (the crate that bridges
// `nmp-core` and `nmp-signers`) needs to construct `ActorCommand` values to
// push `AddRemoteSigner` / `BunkerHandshakeProgress` back to the actor. The
// `actor` module is crate-private so this re-export is the only path. The
// enum variants themselves are already `pub`.
pub use actor::ActorCommand;
pub use actor::NOSTRCONNECT_DEFAULT_RELAY_URL;

// Re-export the FFI entry-points so any native (non-WASM) Rust-side crate
// — including third-party app crates such as `nmp-app-fixture` and any
// future `nmp-app-*` — can call them directly via the Rust rlib dependency,
// without an `extern "C"` block. The symbols remain `#[no_mangle]` on the
// ffi:: side and are still reachable from Swift/C unchanged.
//
// One door per capability: `nmp_app_publish_signed_event`,
// `nmp_app_publish_signed_event_to`, and `nmp_app_publish_unsigned_event`
// were deleted — every user/app-authored event-producing publish now goes
// through `nmp_app_dispatch_action` under the `nmp.publish` namespace.
// `nmp_app_retry_publish` / `nmp_app_cancel_publish` survive as the
// publish-lifecycle control plane (no event production; the D11 lint
// whitelists them).
//
// Gated on `native` (the default feature) so wasm32 (`--no-default-features`)
// continues to compile without these symbols. The `android-ffi` and
// `test-support` features both already imply `native`, so they inherit this
// surface; the deltas they add are the small blocks below.
#[cfg(feature = "native")]
pub use ffi::{
    nmp_app_add_relay, nmp_app_cancel_publish, nmp_app_claim_profile, nmp_app_close_author,
    nmp_app_close_thread, nmp_app_configure, nmp_app_create_new_account, nmp_app_dispatch_action,
    nmp_app_dispatch_capability, nmp_app_free, nmp_app_free_string, nmp_app_lifecycle_background,
    nmp_app_lifecycle_foreground, nmp_app_new, nmp_app_open_author, nmp_app_open_firehose_tag,
    nmp_app_open_thread, nmp_app_open_timeline, nmp_app_open_uri, nmp_app_register_event_observer,
    nmp_app_register_raw_event_observer, nmp_app_release_profile, nmp_app_remove_relay,
    nmp_app_retry_publish, nmp_app_set_capability_callback, nmp_app_set_lifecycle_callback,
    nmp_app_set_storage_path, nmp_app_set_update_callback, nmp_app_signin_nsec, nmp_app_start,
    nmp_app_unregister_event_observer, nmp_app_unregister_raw_event_observer,
};

// test-support delta: live-bench harnesses and integration test binaries need
// a few extra entry points that production app crates do not — pre-verified
// event injection (used by the S3/S4/S5 throughput harnesses), per-action
// stage acks (used by action-FSM tests), and read-side projection JSON dumps
// (used to assert reducer output without going through the snapshot
// callback). Kept gated on test-support so they don't pollute the
// production-app re-export surface.
//
// `test-support` implies the `native` superset above (it requires `native`
// in practice — every symbol below lives in `ffi::`, and `ffi::` is gated on
// `native`). The 32 overlap symbols are exposed through the `native` block.
#[cfg(all(any(test, feature = "test-support"), feature = "native"))]
pub use ffi::{
    nmp_app_ack_action_stage, nmp_app_inject_pre_verified_events,
    nmp_app_inject_signed_event_json, nmp_app_inject_signed_events, nmp_app_read_projection_json,
};

// android-ffi delta: the Android JNI shim (`nmp-android-ffi`) needs four
// extra entry points the standard native re-export above does not yet expose
// — account removal, bunker sign-in, full-actor stop, and active-account
// switch. The rest of the Android JNI surface is inherited from the
// `native` block above (android-ffi implies native). Re-exporting through
// the rlib is what causes rustc to include the symbol bodies in CGU files;
// without Rust-path references the rlib is consumed at compile time but the
// symbols stay `U` in the cdylib.
#[cfg(feature = "android-ffi")]
pub use ffi::{
    nmp_app_remove_account, nmp_app_signin_bunker, nmp_app_stop, nmp_app_switch_active,
};

// D0: NIP-47 NWC is an app noun — the `nmp_app_wallet_*` FFI symbols are
// gated behind the `wallet` Cargo feature. Re-exported via Rust paths for
// the Android JNI shim only when both features are on. `wallet` implies
// `native` implies `android-ffi` already has the `ffi` module available.
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
///
/// V-01 Phase 1c: the facade re-exports `run_actor` and the conformance
/// harness — both live on the native runtime — so the whole module is gated
/// behind `native` as well. Under `--no-default-features` there is no actor
/// thread to spawn and no harness handlers to drive.
#[cfg(all(any(test, feature = "test-support"), feature = "native"))]
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
        // Hand the actor a clone of the command sender so dispatch arms
        // that spawn workers (currently the LNURL-pay round-trip) can
        // send follow-up `ActorCommand`s back into the loop. The outer
        // returned `command_tx` is the host's primary handle; this clone
        // serves only the actor's internal self-feedback path.
        let actor_command_tx_self = command_tx.clone();
        thread::spawn(move || run_actor(command_rx, actor_command_tx_self, update_tx));
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
