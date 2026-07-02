//! Host-side NIP-17 DM runtime registration.

use std::sync::{Arc, Mutex};

use nmp_core::actor::{ActorCommand, InterestsCommand, PublishCommand};
use nmp_core::substrate::{
    ConfiguredRelaysChangeRegistrar, HostCapabilities, IdentityChangeRegistrar,
    IngestParserRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::{read_eligible_relay_urls, AppRelaySlot};

use crate::{
    active_giftwrap_inbox_identity, active_giftwrap_inbox_interest, peer_dm_relay_list_identity,
    peer_dm_relay_list_interest, DmInboxProjection, DmRelayList, DmRuntimeEffect, DmRuntimeState,
    DM_INBOX_FILE_IDENTIFIER, DM_INBOX_INGEST_SLOT, DM_INBOX_SCHEMA_ID, DM_INBOX_SCHEMA_VERSION,
    DM_RELAY_LIST_FILE_IDENTIFIER, DM_RELAY_LIST_SCHEMA_ID, DM_RELAY_LIST_SCHEMA_VERSION,
};

const DM_RELAY_LIST_PROJECTION_KEY: nmp_ownership::DeclaredProjectionKey =
    nmp_ownership::DeclaredProjectionKey::framework(
        "nmp.nip17.dm_relay_list",
        "projection.nmp.nip17.dm_relay_list",
    );
const DM_INBOX_PROJECTION_KEY: nmp_ownership::DeclaredProjectionKey =
    nmp_ownership::DeclaredProjectionKey::framework(
        "nmp.nip17.dm_inbox",
        "projection.nmp.nip17.dm_inbox",
    );

/// Wire the NIP-17 DM runtime into `app`.
///
/// Registers [`DmInboxProjection`] as an `IngestParser` for kind:1059
/// under [`DM_INBOX_INGEST_SLOT`] + the `"nmp.nip17.dm_inbox"` snapshot
/// projection, then captures a [`DmRuntimeController`] that:
///
/// 1. Drives reconciliation (active-account gift-wrap inbox interest +
///    kind:10050 relay-list publishes) from account, relay-list, and inbox
///    ingest events. This is a pure side-effect path; it produces no
///    projection data.
/// 2. Emits the `"nmp.nip17.dm_relay_list"` typed FlatBuffers sidecar
///    via a **separate** typed projection closure that is a PURE READ.
///
/// Keeping the two concerns on separate seams means the projection closure
/// is guaranteed side-effect-free (D0) and the reconciler fires regardless
/// of whether any host consumes the projection.
pub(crate) fn register_runtime(
    app: &(impl HostCapabilities
          + IdentityChangeRegistrar
          + ConfiguredRelaysChangeRegistrar
          + IngestParserRegistrar
          + SnapshotProjectionRegistrar),
) {
    let inbox_projection = register_inbox_projection(app);

    let controller = Arc::new(DmRuntimeController {
        relay_slot: app.configured_relays_handle(),
        active_pubkey: app.active_pubkey(),
        event_store: app.event_store_handle(),
        tx: app.actor_sender(),
        state: Mutex::new(DmRuntimeState::default()),
        inbox_projection,
    });

    let controller_for_identity = Arc::clone(&controller);
    app.register_identity_change_observer(move |_| controller_for_identity.sync());
    let controller_for_relays = Arc::clone(&controller);
    app.register_configured_relays_change_observer(move || controller_for_relays.sync());
    let controller_for_ingest = Arc::clone(&controller);
    app.replace_ingest_parser(
        1059_u32,
        DM_INBOX_INGEST_SLOT,
        Arc::new(DmRuntimeIngestParser {
            projection: Arc::clone(&controller_for_ingest.inbox_projection),
            controller: controller_for_ingest,
        }) as Arc<dyn nmp_core::substrate::IngestParser>,
    );
    controller.sync();

    let controller_typed = Arc::clone(&controller);
    app.register_typed_snapshot_projection(DM_RELAY_LIST_PROJECTION_KEY, move || {
        let relay_list = controller_typed.typed_relay_list();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip17.dm_relay_list".to_string(),
            schema_id: DM_RELAY_LIST_SCHEMA_ID.to_string(),
            schema_version: DM_RELAY_LIST_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(DM_RELAY_LIST_FILE_IDENTIFIER).into_owned(),
            payload: crate::encode_dm_relay_list(&relay_list),
            ..Default::default()
        })
    });
}

fn register_inbox_projection(
    app: &(impl HostCapabilities
          + IdentityChangeRegistrar
          + IngestParserRegistrar
          + SnapshotProjectionRegistrar),
) -> Arc<DmInboxProjection> {
    let active_pubkey = app.active_pubkey();
    let projection = Arc::new(DmInboxProjection::new(
        app.actor_sender(),
        Arc::clone(&active_pubkey),
    ));

    app.replace_ingest_parser(
        1059_u32,
        DM_INBOX_INGEST_SLOT,
        Arc::clone(&projection) as Arc<dyn nmp_core::substrate::IngestParser>,
    );

    let controller = Arc::new(DmInboxController {
        active_pubkey: Arc::clone(&active_pubkey),
        last_seen_pubkey: Mutex::new(active_pubkey.lock().ok().and_then(|slot| slot.clone())),
        projection: Arc::clone(&projection),
    });
    let controller_for_identity = Arc::clone(&controller);
    app.register_identity_change_observer(move |_| {
        controller_for_identity.on_account_change();
    });

    let projection_typed = Arc::clone(&projection);
    app.register_typed_snapshot_projection(DM_INBOX_PROJECTION_KEY, move || {
        let snapshot = projection_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip17.dm_inbox".to_string(),
            schema_id: DM_INBOX_SCHEMA_ID.to_string(),
            schema_version: DM_INBOX_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(DM_INBOX_FILE_IDENTIFIER).into_owned(),
            payload: crate::encode_dm_inbox_snapshot(&snapshot),
            ..Default::default()
        })
    });

    projection
}

struct DmRuntimeIngestParser {
    projection: Arc<DmInboxProjection>,
    controller: Arc<DmRuntimeController>,
}

impl nmp_core::substrate::IngestParser for DmRuntimeIngestParser {
    fn parse(&self, evt: &nmp_store::VerifiedEvent) {
        self.parse_at_source(evt, 0, None);
    }

    fn parse_at_source(
        &self,
        evt: &nmp_store::VerifiedEvent,
        now_secs: u64,
        source_relay_url: Option<&str>,
    ) {
        <DmInboxProjection as nmp_core::substrate::IngestParser>::parse_at_source(
            &self.projection,
            evt,
            now_secs,
            source_relay_url,
        );
        self.controller.sync();
    }
}

pub(crate) struct DmInboxController {
    active_pubkey: nmp_core::slots::ActiveAccountSlot,
    last_seen_pubkey: Mutex<Option<String>>,
    projection: Arc<DmInboxProjection>,
}

impl DmInboxController {
    #[cfg(test)]
    pub(crate) fn new(
        active_pubkey: nmp_core::slots::ActiveAccountSlot,
        tx: nmp_core::CommandSender,
    ) -> Self {
        let initial_pubkey = active_pubkey.lock().ok().and_then(|slot| slot.clone());
        let projection = Arc::new(DmInboxProjection::new(tx, Arc::clone(&active_pubkey)));
        Self {
            active_pubkey,
            last_seen_pubkey: Mutex::new(initial_pubkey),
            projection,
        }
    }

    #[cfg(test)]
    pub(crate) fn inbox_slot(&self) -> Arc<DmInboxProjection> {
        Arc::clone(&self.projection)
    }

    pub(crate) fn on_account_change(&self) -> bool {
        let current = self.active_pubkey.lock().ok().and_then(|slot| slot.clone());

        let mut last = self
            .last_seen_pubkey
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if *last == current {
            return false;
        }

        *last = current;
        self.projection.clear();
        true
    }
}

struct DmRuntimeController {
    relay_slot: AppRelaySlot,
    active_pubkey: nmp_core::slots::ActiveAccountSlot,
    event_store: nmp_core::slots::EventStoreSlot,
    tx: nmp_core::CommandSender,
    state: Mutex<DmRuntimeState>,
    inbox_projection: Arc<DmInboxProjection>,
}

impl DmRuntimeController {
    pub(crate) fn sync(&self) {
        let _ = self
            .inbox_projection
            .launch_batch_backfill(&self.event_store);
        let relay_list = self.typed_relay_list();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for effect in state.reconcile(
            relay_list.active_pubkey.as_deref(),
            &relay_list.read_relay_urls,
            &self.dm_peer_pubkeys(),
        ) {
            self.apply(effect);
        }
    }

    pub(crate) fn typed_relay_list(&self) -> DmRelayList {
        DmRelayList {
            active_pubkey: self.active_pubkey(),
            read_relay_urls: self.read_relay_urls(),
        }
    }

    fn active_pubkey(&self) -> Option<String> {
        self.active_pubkey.lock().ok().and_then(|slot| slot.clone())
    }

    fn read_relay_urls(&self) -> Vec<String> {
        self.relay_slot
            .lock()
            .map(|rows| read_eligible_relay_urls(rows.as_slice()))
            .unwrap_or_default()
    }

    fn dm_peer_pubkeys(&self) -> Vec<String> {
        self.inbox_projection
            .snapshot()
            .conversations
            .into_iter()
            .map(|conversation| conversation.peer_pubkey)
            .collect()
    }

    fn apply(&self, effect: DmRuntimeEffect) {
        let cmd = match effect {
            DmRuntimeEffect::PushInboxInterest(pubkey) => {
                ActorCommand::Interests(InterestsCommand::EnsureInterest {
                    identity: active_giftwrap_inbox_identity(),
                    interest: active_giftwrap_inbox_interest(&pubkey),
                })
            }
            DmRuntimeEffect::WithdrawInboxInterest => ActorCommand::Interests(
                InterestsCommand::DropInterestOwner(active_giftwrap_inbox_identity()),
            ),
            DmRuntimeEffect::PushOwnRelayListInterest(pubkey) => {
                ActorCommand::Interests(InterestsCommand::EnsureInterest {
                    identity: peer_dm_relay_list_identity(&pubkey),
                    interest: peer_dm_relay_list_interest(&pubkey),
                })
            }
            DmRuntimeEffect::WithdrawOwnRelayListInterest(pubkey) => ActorCommand::Interests(
                InterestsCommand::DropInterestOwner(peer_dm_relay_list_identity(&pubkey)),
            ),
            DmRuntimeEffect::PushPeerRelayListInterest(pubkey) => {
                ActorCommand::Interests(InterestsCommand::EnsureInterest {
                    identity: peer_dm_relay_list_identity(&pubkey),
                    interest: peer_dm_relay_list_interest(&pubkey),
                })
            }
            DmRuntimeEffect::WithdrawPeerRelayListInterest(pubkey) => ActorCommand::Interests(
                InterestsCommand::DropInterestOwner(peer_dm_relay_list_identity(&pubkey)),
            ),
            DmRuntimeEffect::PublishRelayList { event, .. } => {
                ActorCommand::Publish(PublishCommand::UnsignedEvent {
                    event,
                    correlation_id: None,
                    signer_pubkey: None,
                })
            }
        };
        let _ = self.tx.send(cmd);
    }
}

#[cfg(test)]
#[path = "runtime_dm_inbox_tests.rs"]
mod dm_inbox_tests;

#[cfg(test)]
#[path = "runtime_dm_relay_list_tests.rs"]
mod dm_relay_list_tests;
