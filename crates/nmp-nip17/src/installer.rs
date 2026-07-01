//! Host-side NIP-17 DM runtime installer.
//!
//! `register_dm_runtime` wires the kind:1059 DM inbox parser/projection and
//! event observers that reconcile gift-wrap inbox interests, kind:10050 relay
//! list hydration, and own relay-list publishes. The paired
//! `"nmp.nip17.dm_relay_list"` typed projection is a pure read.
//!
//! The DM controller degrades silently on lock poisoning or channel disconnect
//! (D6) and uses account/relay/ingest events for effects.

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::actor::{InterestsCommand, PublishCommand};
use nmp_core::substrate::{
    ConfiguredRelaysChangeRegistrar, HostCapabilities, IdentityChangeRegistrar,
    IngestParserRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::{read_eligible_relay_urls, AppRelaySlot};

use crate::{
    active_giftwrap_inbox_identity, active_giftwrap_inbox_interest, peer_dm_relay_list_identity,
    peer_dm_relay_list_interest, DmInboxProjection, DmRuntimeEffect, DmRuntimeState,
    DM_INBOX_INGEST_SLOT,
};

/// Wire the NIP-17 DM runtime into `app`.
///
/// Registers [`DmInboxProjection`] as an `IngestParser` for kind:1059
/// under slot `"nip17.dm_inbox"` + the `"nmp.nip17.dm_inbox"` snapshot
/// projection, then captures a `DmRuntimeController` that:
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
pub fn register_dm_runtime(
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

    // Typed FlatBuffers sidecar (ADR-0037, Wave A): PURE READ — reads the
    // relay-list state and encodes it. Reconciliation (push/withdraw) is
    // handled exclusively by event observers above so this closure is
    // side-effect-free (D0).
    let controller_typed = Arc::clone(&controller);
    app.register_typed_snapshot_projection("nmp.nip17.dm_relay_list", move || {
        let relay_list = controller_typed.typed_relay_list();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip17.dm_relay_list".to_string(),
            schema_id: crate::DM_RELAY_LIST_SCHEMA_ID.to_string(),
            schema_version: crate::DM_RELAY_LIST_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(crate::DM_RELAY_LIST_FILE_IDENTIFIER)
                .into_owned(),
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
    app.register_typed_snapshot_projection("nmp.nip17.dm_inbox", move || {
        let snapshot = projection_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip17.dm_inbox".to_string(),
            schema_id: crate::DM_INBOX_SCHEMA_ID.to_string(),
            schema_version: crate::DM_INBOX_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(crate::DM_INBOX_FILE_IDENTIFIER).into_owned(),
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

/// Lifecycle controller for the DM inbox projection.
///
/// Detects active-account changes and clears the inbox projection so
/// decrypted DMs from the previous account cannot leak into the new
/// account's snapshot (issue #1138). The projection `Arc` is shared
/// with the snapshot closures registered in `register_inbox_projection`.
pub struct DmInboxController {
    /// Pubkey-only active-account slot — read to determine the current pubkey
    /// (populated for every backend including bunker; ADR-0050 §D6).
    active_pubkey: nmp_core::slots::ActiveAccountSlot,
    /// Last pubkey we observed — used to detect genuine account changes
    /// (vs. spurious identity-observer firings).
    last_seen_pubkey: Mutex<Option<String>>,
    /// The live projection, shared with both snapshot closures. `clear()` is
    /// called on this when an account change is detected.
    projection: Arc<DmInboxProjection>,
}

impl DmInboxController {
    /// Construct a controller bound to the pubkey-only active-account slot and
    /// command sender, seeding the last-seen pubkey from the slot's current
    /// value so the first observer fire is not a false positive.
    pub fn new(
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

    /// Return a clone of the shared projection `Arc`.
    pub fn inbox_slot(&self) -> Arc<DmInboxProjection> {
        Arc::clone(&self.projection)
    }

    /// Called by the identity-change observer when the active account may have
    /// changed. Returns `true` when a genuine account change was detected and
    /// the projection was cleared; `false` for a no-op.
    ///
    /// D6 — a poisoned `last_seen_pubkey` mutex is treated as "no prior
    /// account" so the next sign-in still clears (safe over-clear).
    pub fn on_account_change(&self) -> bool {
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
    /// Event-driven reconciler.
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

    /// Read the current `(active_pubkey, read_relay_urls)` into the SINGLE
    /// `crate::DmRelayList` read model. A PURE READ.
    pub(crate) fn typed_relay_list(&self) -> crate::DmRelayList {
        crate::DmRelayList {
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

// DM inbox account-switch teardown tests — verifies issue #1138 fix.
#[cfg(test)]
#[path = "installer_dm_inbox_tests.rs"]
mod dm_inbox_tests;

// DM relay-list reconciler bunker-activation tests (Finding C) — verifies the
// `DmRuntimeController` activates from the pubkey-only slot.
#[cfg(test)]
#[path = "installer_dm_relay_list_tests.rs"]
mod dm_relay_list_tests;
