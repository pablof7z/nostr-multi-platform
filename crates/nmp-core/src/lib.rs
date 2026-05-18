mod actor;
mod app;
mod ffi;
mod kernel;
pub mod planner;
pub mod publish;
mod relay;
mod relay_worker;
pub mod store;
pub mod substrate;

pub use app::{AppState, KernelAction, KernelUpdate, KernelViewSpec};
pub use ffi::NmpApp;

// Re-export the FFI entry-points so the ffi-stress harness (and any other
// Rust-side crate) can call them directly via the Rust rlib dependency,
// without an `extern "C"` block. The symbols remain `#[no_mangle]` on the
// ffi:: side and are still reachable from Swift/C unchanged.
#[cfg(any(test, feature = "test-support"))]
pub use ffi::{
    nmp_app_claim_profile, nmp_app_close_author, nmp_app_configure, nmp_app_free,
    nmp_app_inject_pre_verified_events, nmp_app_inject_signed_events, nmp_app_new,
    nmp_app_open_author, nmp_app_open_firehose_tag, nmp_app_release_profile,
    nmp_app_set_update_callback, nmp_app_start,
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
