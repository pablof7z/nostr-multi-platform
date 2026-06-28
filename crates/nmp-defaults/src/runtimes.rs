//! Canonical host-side runtime controllers wired by [`super::register_defaults`].
//!
//! Runtime controllers own active-account scoped interest bookkeeping the
//! kernel itself cannot do (D0 — `nmp-core` ships no DM/zap nouns).
//!
//! `register_dm_runtime` wires the kind:1059 DM inbox parser/projection and a
//! event observers that reconcile gift-wrap inbox interests, kind:10050 relay
//! list hydration, and own relay-list publishes. The paired
//! `"nmp.nip17.dm_relay_list"` typed projection is a pure read.
//!
//! `register_zap_receipts_runtime` wires the NIP-57 self-zap receipt identity
//! observer. Both controllers degrade silently on lock poisoning or channel
//! disconnect (D6) and use account/relay/ingest events for effects.

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::actor::{InterestsCommand, PublishCommand};
use nmp_core::substrate::{
    ConfiguredRelaysChangeRegistrar, HostCapabilities, IdentityChangeRegistrar,
    IngestParserRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::{read_eligible_relay_urls, AppRelaySlot};
use nmp_nip17::{
    active_giftwrap_inbox_identity, active_giftwrap_inbox_interest, peer_dm_relay_list_identity,
    peer_dm_relay_list_interest, DmInboxProjection, DmRuntimeEffect, DmRuntimeState,
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
///
/// Called by [`super::register_defaults`]; exposed `pub` so an app crate
/// that opts out of the wholesale defaults can still wire just the DM
/// runtime by itself.
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
        // Pubkey-only identity (Finding C): the relay-list reconciler only
        // needs the active pubkey, never secret keys — read the slot the kernel
        // populates for every backend so bunker accounts reconcile their inbox
        // interest + kind:10050 relay list too.
        active_pubkey: app.active_pubkey(),
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
        nmp_nip17::DM_INBOX_INGEST_SLOT,
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
            schema_id: nmp_nip17::DM_RELAY_LIST_SCHEMA_ID.to_string(),
            schema_version: nmp_nip17::DM_RELAY_LIST_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(nmp_nip17::DM_RELAY_LIST_FILE_IDENTIFIER)
                .into_owned(),
            payload: nmp_nip17::encode_dm_relay_list(&relay_list),
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
    // Raw-tap retirement ladder complete (rules A5, PR-1 + PR-2): the DM inbox
    // projection rides the substrate `IngestParser` seam exclusively.
    //
    // Issue #1138 — cross-account privacy leak: without the fix below,
    // `DmInboxProjection` held onto decrypted messages across an account switch
    // (the `messages` map was never cleared). The fix registers an
    // identity-change observer (same seam as `op_feed_defaults.rs:308`) that
    // calls `DmInboxProjection::clear()` whenever the active account changes, so
    // the previous account's DMs are dropped before the new account's snapshot
    // is served. Structurally impossible to leak them further.
    //
    // Cache-serve feeds the IngestParser exclusively via
    // `EventIngestDispatcher::dispatch`; the raw-event tap is no longer used for
    // the DM inbox or Marmot (Marmot rides its own `"marmot"` slot parser).
    //
    // ADR-0050 §D6 — gift-UNWRAP routes through the signer port: the projection
    // takes the actor command sender + pubkey-only active-account slot (NOT raw
    // `nostr::Keys`, D13), issuing `Nip44DecryptForAccount` so a bunker account
    // can unseal a gift-wrap (V-08). The identity observer drives `clear()`.
    let active_pubkey = app.active_pubkey();
    let projection = Arc::new(DmInboxProjection::new(
        app.actor_sender(),
        Arc::clone(&active_pubkey),
    ));

    // Register as IngestParser for kind:1059 (NIP-59 gift-wrap), under the
    // typed DM_INBOX_INGEST_SLOT key (#1724). Slot-keyed replace ensures only the
    // prior DM inbox parser is evicted on account switch — Marmot's
    // MARMOT_INGEST_SLOT parser is untouched.
    // The kind literal is 1059 per NIP-59; nmp-defaults is a composition crate
    // entitled to name NIP kind numbers directly.
    app.replace_ingest_parser(
        1059_u32, // kind:1059 — NIP-59 gift-wrap
        nmp_nip17::DM_INBOX_INGEST_SLOT,
        Arc::clone(&projection) as Arc<dyn nmp_core::substrate::IngestParser>,
    );

    // ── #1138 fix: clear inbox on account switch ─────────────────────────
    //
    // The identity-change observer fires after the actor writes the active
    // account slot. It compares the new active pubkey (ADR-0050 §D6: read from
    // the pubkey-only `ActiveAccountSlot`, populated for EVERY backend including
    // bunker — the old keys slot was dead for bunker) to the last-seen one; on a
    // change it calls `DmInboxProjection::clear()`, which bumps the §D6 epoch so
    // the previous account's messages cannot appear in a subsequent `snapshot()`
    // AND any in-flight decrypt chain discards its terminal insert. The shared
    // `projection` Arc makes `clear()` visible to both snapshot closures on the
    // next tick (the observer fires before it — no gap window). D6: a poisoned
    // mutex degrades `clear()` silently.
    let controller = Arc::new(DmInboxController {
        active_pubkey: Arc::clone(&active_pubkey),
        last_seen_pubkey: Mutex::new(
            // Seed with the registration-time active pubkey so the first
            // identity-change fire is not a false positive (same pattern as
            // `op_feed_defaults.rs:290` — seed `last_seen` at construction).
            active_pubkey.lock().ok().and_then(|slot| slot.clone()),
        ),
        projection: Arc::clone(&projection),
    });
    let controller_for_identity = Arc::clone(&controller);
    app.register_identity_change_observer(move |_| {
        controller_for_identity.on_account_change();
    });

    // Typed FlatBuffers sidecar (ADR-0037, Wave A), registered ALONGSIDE the
    // generic `Value` projection under the same key (additive — a `NDMI`-aware
    // host prefers it, others fall back). Clone the `Arc` first: the generic
    // closure below consumes `projection`.
    let projection_typed = Arc::clone(&projection);
    app.register_typed_snapshot_projection("nmp.nip17.dm_inbox", move || {
        let snapshot = projection_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip17.dm_inbox".to_string(),
            schema_id: nmp_nip17::DM_INBOX_SCHEMA_ID.to_string(),
            schema_version: nmp_nip17::DM_INBOX_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(nmp_nip17::DM_INBOX_FILE_IDENTIFIER)
                .into_owned(),
            payload: nmp_nip17::encode_dm_inbox_snapshot(&snapshot),
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
pub(crate) struct DmInboxController {
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
    ///
    /// Exposed `pub(crate)` so the unit tests in `runtimes_dm_inbox_tests` can
    /// construct a controller without a real `AppHost`.
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

    /// Return a clone of the shared projection `Arc`.
    ///
    /// Used by the unit tests to feed DMs into the live projection and inspect
    /// the snapshot.
    pub(crate) fn inbox_slot(&self) -> Arc<DmInboxProjection> {
        Arc::clone(&self.projection)
    }

    /// Called by the identity-change observer when the active account may have
    /// changed. Compares the current pubkey in `active_pubkey` to the last-seen
    /// value; if they differ, clears the projection and updates the cache.
    ///
    /// Returns `true` when a genuine account change was detected and the
    /// projection was cleared; `false` for a no-op (same pubkey or poisoned
    /// mutex).
    ///
    /// D6 — a poisoned `last_seen_pubkey` mutex is treated as "no prior
    /// account" so the next sign-in still clears (safe over-clear, not an
    /// under-clear).
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
    /// Pubkey-only identity slot (Finding C): the active account's hex pubkey,
    /// populated for every backend including bunker. The reconciler needs
    /// identity only, never secret key material.
    active_pubkey: nmp_core::slots::ActiveAccountSlot,
    tx: nmp_core::CommandSender,
    state: Mutex<DmRuntimeState>,
    inbox_projection: Arc<DmInboxProjection>,
}

impl DmRuntimeController {
    /// Event-driven reconciler.
    ///
    /// Reads the current relay-list state, runs [`DmRuntimeState::reconcile`]
    /// against it, and applies any resulting effects (push/withdraw inbox
    /// interest, publish relay-list event). This is the ONLY path that emits
    /// actor commands from the DM relay-list runtime; the typed projection
    /// closure is a pure read.
    ///
    /// D6: a poisoned state mutex degrades to a no-op (no effects emitted,
    /// no crash on the actor thread). D8: channel send is non-blocking.
    pub(crate) fn sync(&self) {
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
    /// `nmp_nip17::DmRelayList` read model. A PURE READ — it deliberately does
    /// NOT run `reconcile`, so the push/withdraw actor traffic stays driven
    /// exclusively by [`Self::sync`]. The typed projection closure is
    /// side-effect-free and mirrors the reconciler's read model.
    pub(crate) fn typed_relay_list(&self) -> nmp_nip17::DmRelayList {
        nmp_nip17::DmRelayList {
            active_pubkey: self.active_pubkey(),
            read_relay_urls: self.read_relay_urls(),
        }
    }

    fn active_pubkey(&self) -> Option<String> {
        // Identity straight from the pubkey slot — already hex, no keypair
        // derivation. `None` on a poisoned lock or no signed-in account.
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
                // Non-dispatch internal path — the action-seam variant at
                // `nmp_nip17::PublishDmRelayListAction::execute` is where
                // the dispatch-side correlation_id round-trip happens.
                ActorCommand::Publish(PublishCommand::UnsignedEvent {
                    event,
                    correlation_id: None,
                    // Internal DM-relay-list publish signs with the active account.
                    signer_pubkey: None,
                })
            }
        };
        let _ = self.tx.send(cmd);
    }
}

// ───────────────────────────────────────────────────────────────────────
// NIP-51 mute-list runtime
// ───────────────────────────────────────────────────────────────────────
//
// Extracted to `runtimes/mute_runtime.rs` to hold this module under the 500-LOC
// hard ceiling (AGENTS.md: extract, never bump the baseline). Re-exported here so
// `runtimes::register_mute_runtime` (and the `nmp_defaults::register_mute_runtime`
// facade in `lib.rs`) stay unchanged.
mod mute_runtime;
pub use mute_runtime::register_mute_runtime;

mod bookmarks_runtime;
pub use bookmarks_runtime::{
    register_bookmark_runtime, register_bookmark_set_runtime, register_web_bookmark_runtime,
};

mod comments_runtime;
pub use comments_runtime::register_comment_runtime;

// ───────────────────────────────────────────────────────────────────────
// NIP-51 search-relay-list runtime
// ───────────────────────────────────────────────────────────────────────
//
// Extracted to `runtimes/search_relay_runtime.rs` to hold this module under
// the 500-LOC hard ceiling (AGENTS.md: extract, never bump the baseline).
// Re-exported here so `runtimes::register_search_relay_runtime` (and the
// `nmp_defaults::register_search_relay_runtime` facade in `lib.rs`) stay at
// a stable path.
mod search_relay_runtime;
pub use search_relay_runtime::{register_search_relay_runtime, register_search_relay_runtime_with};

mod zap_receipts_runtime;
pub use zap_receipts_runtime::register_zap_receipts_runtime;

// Mute-list active observed-projection reconciler tests.
#[cfg(test)]
#[path = "runtimes_mute_tests.rs"]
mod mute_tests;

// DM inbox account-switch teardown tests — verifies issue #1138 fix.
#[cfg(test)]
#[path = "runtimes_dm_inbox_tests.rs"]
mod dm_inbox_tests;

// DM relay-list reconciler bunker-activation tests (Finding C) — verifies the
// `DmRuntimeController` activates from the pubkey-only slot.
#[cfg(test)]
#[path = "runtimes_dm_relay_list_tests.rs"]
mod dm_relay_list_tests;
