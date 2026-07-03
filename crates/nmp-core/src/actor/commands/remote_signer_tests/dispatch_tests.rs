#![cfg(test)]
//! End-to-end dispatch tests that drive `ActorCommand` variants through the
//! spawned `run_actor` / `run_actor_with_observers` loop so the dispatch arms
//! are exercised (not just the command-handler functions they wrap).

use crate::actor::{IdentityCommand, LifecycleCommand};
use nmp_signer_iface::RemoteSignerHandle;

use super::stub_signer;

// ──────────────────────────────────────────────────────────────────────────
// End-to-end dispatch test — drives the new `ActorCommand` variants through
// the spawned `run_actor` loop so the dispatch arms are exercised (not just
// the command-handler functions they wrap).
// ──────────────────────────────────────────────────────────────────────────

/// PR-B (#991/#979): drain snapshot frames from the channel and return the typed
/// sidecar entries of the LAST snapshot frame received. The generic JSON `payload`
/// is no longer emitted on the wire after payload zeroing; callers must use the
/// typed sidecar decoders.
pub(super) fn last_typed_sidecars(
    upd_rx: &std::sync::mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
) -> Vec<crate::update_envelope::TypedProjectionData> {
    let mut last: Vec<crate::update_envelope::TypedProjectionData> = Vec::new();
    while let Ok(frame) = upd_rx.try_recv() {
        if let Ok(typed) = crate::update_envelope::decode_snapshot_typed_projections(&frame) {
            last = typed;
        }
    }
    last
}

#[test]
fn snapshot_carries_nip46_onboarding_projection() {
    // The built-in `"nip46_onboarding"` projection is wired alongside
    // `"bunker_handshake"` and produces a typed DTO with the static
    // signer-app table + pre-computed flags. This end-to-end test drives
    // a `BunkerHandshakeProgress` through the actor and asserts both
    // projections appear in the emitted snapshot.
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{
        run_actor_with_observers, ActorChannels, ActorCommand, ActorConfigSources, ActorMail,
        ActorRuntimeSlots, CommandSender,
    };
    use crate::capability_socket::new_capability_callback_slot;

    let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
    let cmd_tx = CommandSender::new(inbox_tx);
    let (upd_tx, upd_rx) = mpsc::channel::<crate::update_envelope::UpdateFrameBytes>();

    let snapshot_projections = crate::kernel::new_snapshot_projection_slot();
    let bunker_slot = crate::actor::new_bunker_handshake_slot();
    // Wire the two NIP-46 typed projections exactly as the actor does.
    {
        let typed_slot = Arc::clone(&bunker_slot);
        snapshot_projections
            .lock()
            .expect("registry lock")
            .register_typed("nip46_onboarding", move || {
                crate::actor::typed_projections::nip46_onboarding_typed(&typed_slot)
            });
    }

    let actor_self_tx = cmd_tx.clone();
    thread::spawn(move || {
        let runtime = ActorRuntimeSlots {
            lifecycle_observer: crate::actor::new_lifecycle_observer_slot(),
            event_observers: crate::actor::new_event_observer_slot(),
            snapshot_projections,
            bunker_handshake: bunker_slot,
            signer_state: crate::actor::new_signer_state_slot(),
            bunker_hook: crate::new_bunker_hook_slot(),
            external_signer_hook: crate::new_external_signer_hook_slot(),
            configured_relays: crate::kernel::new_app_relay_slot(),
            mls_local_nsec: Arc::new(std::sync::Mutex::new(None)),
            active_local_keys: Arc::new(std::sync::Mutex::new(None)),
            capability_callback: new_capability_callback_slot(),
            queue_depth: Arc::new(AtomicU64::new(0)),
            routing_trace: Arc::new(std::sync::Mutex::new(None)),
            active_account: crate::slots::new_active_account_slot(),
            event_store: crate::slots::new_event_store_slot(),
            pull_cursor_registry: crate::slots::new_pull_cursor_registry_handle_slot(),
            external_event_sink_dispatcher:
                crate::substrate::new_external_event_sink_dispatcher_slot(),
        };
        let config = ActorConfigSources {
            storage_path: Arc::new(std::sync::Mutex::new(None)),
            coverage_hook: Arc::new(std::sync::Mutex::new(None)),
            req_frame_interceptor: crate::substrate::new_req_frame_interceptor_slot(),
            host_op_handler: crate::substrate::new_host_op_handler_slot(),
            relay_text_interceptor: crate::substrate::new_relay_text_interceptor_slot(),
            relay_connected_hook: crate::substrate::new_relay_connected_hook_slot(),
            ingest_dispatcher: Arc::new(std::sync::RwLock::new(
                crate::substrate::EventIngestDispatcher::new(),
            )),
            search_scope_registry: Arc::new(crate::substrate::SearchScopeRegistry::new()),
            dm_inbox_relays: Arc::new(std::sync::Mutex::new(
                crate::substrate::empty_dm_inbox_relay_lookup(),
            )),
            contact_list_reader: crate::slots::new_contact_list_reader_slot(),
            profile_lookup: Arc::new(std::sync::Mutex::new(
                crate::substrate::empty_profile_lookup(),
            )),
            blocked_relays: Arc::new(std::sync::Mutex::new(
                crate::substrate::empty_blocked_relay_lookup(),
            )),
            bootstrap_self_kinds: Arc::new(std::sync::Mutex::new(None)),
            routing_substrate: crate::slots::new_routing_substrate_slot(),
            publish_resolver: crate::slots::new_publish_resolver_slot(),
            relay_list_publish_support: crate::slots::new_relay_list_publish_support_slot(),
            external_event_sink_policy: crate::slots::new_external_event_sink_policy_slot(),
            kernel_clock: crate::slots::new_kernel_clock_slot(),
            gc_budget_ceiling: None,
            user_agent: Arc::new(std::sync::Mutex::new(None)),
            outbound_public_tags: Arc::new(std::sync::Mutex::new(None)),
        }
        .snapshot();
        run_actor_with_observers(
            ActorChannels {
                inbox_rx: cmd_rx,
                command_tx_self: actor_self_tx,
                update_tx: upd_tx,
            },
            config,
            runtime,
        );
    });

    cmd_tx
        .send(ActorCommand::Lifecycle(LifecycleCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
            initial_relays: Vec::new(),
        }))
        .unwrap();

    cmd_tx
        .send(ActorCommand::Identity(
            IdentityCommand::BunkerHandshakeProgress {
                stage: "connecting".to_string(),
                code: None,
                message: Some("dialing relay".to_string()),
            },
        ))
        .unwrap();

    thread::sleep(Duration::from_millis(300));
    let _ = cmd_tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown));

    // PR-B (#991/#979): payload is zeroed — read from the typed sidecar instead.
    let sidecars = last_typed_sidecars(&upd_rx);
    assert!(!sidecars.is_empty(), "actor produced no snapshot frames");

    // Decode the nip46_onboarding typed sidecar.
    let onboarding_entry = sidecars
        .iter()
        .find(|p| p.key == crate::actor::typed_projections::NIP46_ONBOARDING_SCHEMA_ID)
        .expect("snapshot missing nip46_onboarding typed sidecar");
    let onboarding =
        crate::actor::typed_projections::decode_nip46_onboarding(&onboarding_entry.payload)
            .expect("nip46_onboarding sidecar must decode");

    // The typed projection's `stage_kind` + `is_in_flight` must reflect the
    // same broker progress as the prior JSON path.
    assert_eq!(
        onboarding.stage_kind.as_deref(),
        Some("connecting"),
        "nip46_onboarding must carry stage_kind=connecting"
    );
    assert!(
        onboarding.is_in_flight,
        "nip46_onboarding must pre-compute is_in_flight=true for connecting"
    );
    assert!(
        !onboarding.signer_apps.is_empty(),
        "nip46_onboarding must carry non-empty signer_apps table"
    );
}

#[test]
fn dispatch_add_remote_signer_then_progress_surfaces_on_snapshot() {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{spawn_test_actor, ActorCommand, ActorMail, CommandSender};

    let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
    let cmd_tx = CommandSender::new(inbox_tx);
    let (upd_tx, upd_rx) = mpsc::channel::<crate::update_envelope::UpdateFrameBytes>();
    let actor_self_tx = cmd_tx.clone();
    thread::spawn(move || spawn_test_actor(cmd_rx, actor_self_tx, upd_tx));

    cmd_tx
        .send(ActorCommand::Lifecycle(LifecycleCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
            initial_relays: Vec::new(),
        }))
        .unwrap();

    let (handle, _count) = stub_signer();
    let pk = handle.pubkey_hex();
    cmd_tx
        .send(ActorCommand::Identity(IdentityCommand::AddSigner {
            source: crate::actor::SignerSource::RemoteHandle(handle),
            make_active: true,
        }))
        .unwrap();
    cmd_tx
        .send(ActorCommand::Identity(
            IdentityCommand::BunkerHandshakeProgress {
                stage: "ready".to_string(),
                code: None,
                message: None,
            },
        ))
        .unwrap();

    // Let the actor drain both commands and emit at least one snapshot.
    thread::sleep(Duration::from_millis(300));
    let _ = cmd_tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown));

    // PR-B (#991/#979): payload zeroed — read from the typed sidecar instead.
    let sidecars = last_typed_sidecars(&upd_rx);
    assert!(!sidecars.is_empty(), "actor produced no snapshot frames");

    // Decode the `accounts` typed sidecar and assert the remote-signer pubkey
    // is present and the signer_kind is "nip46".
    let accounts_entry = sidecars
        .iter()
        .find(|p| p.key == crate::kernel::public_typed_projections::ACCOUNTS_SCHEMA_ID)
        .expect("snapshot missing accounts typed sidecar");
    let accounts =
        crate::kernel::public_typed_projections::decode_accounts(&accounts_entry.payload)
            .expect("accounts sidecar must decode");
    assert!(
        accounts
            .accounts
            .iter()
            .any(|row| row.id == pk || row.npub.contains(&pk)),
        "snapshot missing remote-signer pubkey {pk} in accounts sidecar"
    );
    assert!(
        accounts
            .accounts
            .iter()
            .any(|row| row.signer_kind == "nip46"),
        "snapshot missing nip46 signer_kind in accounts sidecar"
    );

    // Decode the `bunker_handshake` typed sidecar and assert stage=ready.
    let bhs_entry = sidecars
        .iter()
        .find(|p| p.key == crate::actor::typed_projections::BUNKER_HANDSHAKE_SCHEMA_ID)
        .expect("snapshot missing bunker_handshake typed sidecar");
    let handshake = crate::actor::typed_projections::decode_bunker_handshake(&bhs_entry.payload)
        .expect("bunker_handshake sidecar must decode");
    assert_eq!(
        handshake.stage, "ready",
        "snapshot missing handshake stage=ready"
    );
}
