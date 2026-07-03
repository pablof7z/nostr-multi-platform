//! Test-support facade for `nmp-core` (extracted from the crate root for
//! file-size ownership). Gated by `test-support`/`native`; re-exports the
//! actor test entrypoints and the NIP golden-tag conformance harness.

pub use crate::actor::{spawn_test_actor, ActorCommand, TestSupportCommand};
pub use crate::kernel::{
    Kernel, PROCESS_PROJECTIONS_CHANGED, PROCESS_PROJECTIONS_SERIALIZED,
    PROCESS_RAM_EVENTS_EVICTED, PROCESS_STORE_LRU_EVICTED,
};
pub use crate::relay::DEFAULT_VISIBLE_LIMIT;
pub use crate::store::{RawEvent, VerifiedEvent}; // ADR-0070 churn

/// NIP golden-tag conformance harness — drives the (crate-private) command
/// handlers against a real `Kernel` + `IdentityRuntime` and returns the
/// emitted `EVENT` JSON so an integration test can assert per-kind tag
/// structure. See `tests/nip_tag_conformance.rs`.
pub use crate::actor::ConformanceHarness;

use std::{sync::mpsc, thread};

/// Spawn the kernel actor on a dedicated thread.
///
/// Returns a command sender and an update receiver.  The caller drives the
/// actor by sending [`ActorCommand`] values and reads FlatBuffers update
/// frames from the update channel.  Dropping the sender or sending
/// [`ActorCommand::Lifecycle(LifecycleCommand::Shutdown)`] stops the actor thread.
pub fn spawn_actor() -> (
    crate::CommandSender,
    mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
) {
    // ADR-0072 / ADR-0072 §D3a — one bounded waking inbox of `ActorMail`.
    // The host handle and the actor's self-feedback handle are both
    // `CommandSender`s over this one channel, so any accepted command wakes the
    // actor without giving the FFI lane unbounded memory.
    let (command_tx, command_rx) = crate::CommandSender::bounded_channel();
    let (update_tx, update_rx) = mpsc::channel();
    // Hand the actor a clone of the command sender so dispatch arms
    // that spawn workers (currently the LNURL-pay round-trip) can
    // send follow-up `ActorCommand`s back into the loop. The outer
    // returned `command_tx` is the host's primary handle; this clone
    // serves only the actor's internal self-feedback path.
    let actor_command_tx_self = command_tx.clone();
    thread::spawn(move || spawn_test_actor(command_rx, actor_command_tx_self, update_tx));
    (command_tx, update_rx)
}

/// Spawn the kernel actor with a pre-set LMDB storage path.
///
/// Identical to [`spawn_actor`] but writes `storage_path` into the slot
/// before the actor thread reads it, so `Kernel::with_storage_path` picks
/// it up at construction time (requires the `lmdb-backend` feature in
/// `nmp-core`).  Used by the W9 A3 restart-persistence acceptance test.
#[cfg(feature = "lmdb-backend")]
pub fn spawn_actor_with_storage_path(
    storage_path: &str,
) -> (
    crate::CommandSender,
    mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
) {
    use crate::actor::{
        run_actor_with_observers, ActorChannels, ActorConfigSources, ActorRuntimeSlots,
    };
    use crate::slots::new_storage_path_slot;
    use std::sync::{atomic::AtomicU64, Arc, Mutex};

    let (command_tx, command_rx) = crate::CommandSender::bounded_channel();
    let (update_tx, update_rx) = mpsc::channel();
    let actor_command_tx_self = command_tx.clone();

    // Pre-populate the storage path slot so the actor reads it at startup.
    let path_slot = new_storage_path_slot();
    *path_slot.lock().expect("storage_path slot") = Some(storage_path.to_string());

    thread::spawn(move || {
        let runtime = ActorRuntimeSlots {
            lifecycle_observer: crate::actor::new_lifecycle_observer_slot(),
            event_observers: crate::actor::new_event_observer_slot(),
            snapshot_projections: crate::kernel::new_snapshot_projection_slot(),
            bunker_handshake: crate::actor::new_bunker_handshake_slot(),
            signer_state: crate::actor::new_signer_state_slot(),
            bunker_hook: crate::new_bunker_hook_slot(),
            external_signer_hook: crate::new_external_signer_hook_slot(),
            configured_relays: crate::kernel::new_app_relay_slot(),
            mls_local_nsec: Arc::new(Mutex::new(None)), // doctrine-allow: D13 — test-support slot init to None (not a raw-key read); was cfg-gated in lib.rs before this module was split out
            active_local_keys: Arc::new(Mutex::new(None)),
            capability_callback: crate::capability_socket::new_capability_callback_slot(),
            queue_depth: Arc::new(AtomicU64::new(0)),
            routing_trace: Arc::new(Mutex::new(None)),
            active_account: crate::slots::new_active_account_slot(),
            event_store: crate::slots::new_event_store_slot(),
            pull_cursor_registry: crate::slots::new_pull_cursor_registry_handle_slot(),
            external_event_sink_dispatcher:
                crate::substrate::new_external_event_sink_dispatcher_slot(),
        };
        let config = ActorConfigSources {
            storage_path: path_slot,
            coverage_hook: Arc::new(Mutex::new(None)),
            req_frame_interceptor: crate::substrate::new_req_frame_interceptor_slot(),
            host_op_handler: crate::substrate::new_host_op_handler_slot(),
            relay_text_interceptor: crate::substrate::new_relay_text_interceptor_slot(),
            relay_connected_hook: crate::substrate::new_relay_connected_hook_slot(),
            ingest_dispatcher: Arc::new(std::sync::RwLock::new(
                crate::substrate::EventIngestDispatcher::new(),
            )),
            search_scope_registry: Arc::new(crate::substrate::SearchScopeRegistry::new()),
            draft_builders: Arc::new(crate::substrate::DraftBuilderRegistry::new()),
            dm_inbox_relays: Arc::new(Mutex::new(crate::substrate::empty_dm_inbox_relay_lookup())),
            contact_list_reader: crate::slots::new_contact_list_reader_slot(),
            profile_lookup: Arc::new(Mutex::new(crate::substrate::empty_profile_lookup())),
            blocked_relays: Arc::new(Mutex::new(crate::substrate::empty_blocked_relay_lookup())),
            bootstrap_self_kinds: Arc::new(Mutex::new(None)),
            user_agent: Arc::new(Mutex::new(None)),
            outbound_public_tags: Arc::new(Mutex::new(None)),
            routing_substrate: crate::slots::new_routing_substrate_slot(),
            publish_resolver: crate::slots::new_publish_resolver_slot(),
            relay_list_publish_support: crate::slots::new_relay_list_publish_support_slot(),
            external_event_sink_policy: crate::slots::new_external_event_sink_policy_slot(),
            kernel_clock: crate::slots::new_kernel_clock_slot(),
            gc_budget_ceiling: None,
        }
        .snapshot();
        run_actor_with_observers(
            ActorChannels {
                inbox_rx: command_rx,
                command_tx_self: actor_command_tx_self,
                update_tx,
            },
            config,
            runtime,
        );
    });
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
    tx: &crate::CommandSender,
    base_ts: u64,
    count: u32,
) -> Result<(), crate::CommandSendError> {
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
    tx.send(ActorCommand::TestSupport(
        TestSupportCommand::IngestPreVerifiedEvents(events),
    ))
    .map(|_| ())
}

/// Send a [`ActorCommand::Barrier`] and block until the actor acknowledges
/// it (V-105). Returns `true` when the ack arrives before `timeout`, or
/// `false` on timeout / disconnected channel.
///
/// Sending `Barrier` after a batch of commands and waiting for the ack is
/// the deterministic replacement for blind `recv_timeout` drain loops:
/// the ack fires only once the actor has dispatched every command that
/// preceded the barrier on the channel, so when `wait_barrier` returns
/// `true` the actor's state reflects all prior commands.
pub fn wait_barrier(tx: &crate::CommandSender, timeout: std::time::Duration) -> bool {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    if tx
        .send(ActorCommand::TestSupport(TestSupportCommand::Barrier {
            ack: ack_tx,
        }))
        .is_err()
    {
        return false;
    }
    ack_rx.recv_timeout(timeout).is_ok()
}
