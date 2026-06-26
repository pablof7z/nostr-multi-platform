//! Canonical host-side runtime controllers wired by [`super::register_defaults`].
//!
//! Two per-tick reconcilers that own active-account scoped interest
//! book-keeping the kernel itself cannot do (D0 — `nmp-core` ships no DM/zap
//! nouns):
//!
//! 1. [`register_dm_runtime`] — NIP-17 DM inbox.
//!    * Wires the kind:1059 [`nmp_nip17::DmInboxProjection`] as an
//!      `IngestParser` under slot `"nip17.dm_inbox"` + its
//!      `"nmp.nip17.dm_inbox"` snapshot projection.
//!    * Owns a `DmRuntimeController` registered via TWO seams:
//!      (a) a **per-tick observer** that reconciles the active-account
//!          gift-wrap inbox interest + pending kind:10050 publishes once
//!          per tick (pure side-effect, no projection data), and
//!      (b) a typed `"nmp.nip17.dm_relay_list"` projection closure that
//!          is a PURE READ of the relay-list state (no reconcile inside).
//! 2. [`register_zap_receipts_runtime`] — NIP-57 self-zap receipts.
//!    * Owns a `ZapReceiptsRuntimeController` registered via the generic
//!      **per-tick observer** seam (`register_snapshot_tick_observer`): it
//!      ensures / drops the active-account kind:9735 `#p` subscription on
//!      sign-in / account switch / sign-out and contributes NO snapshot data
//!      (visible card zap counts are acquired through
//!      `nmp.nip01.visible_note_relations`, not a global zap aggregate).
//!
//! # Both controllers
//!
//! The snapshot tick drives reconciliation — the ensure must happen *before* the
//! first event, the moment the user signs in. Both reconcile against a single
//! `Mutex<Option<String>>` of the last-ensured pubkey, dropping a scoped owner
//! so an account switch cleanly replaces rather than leaks, and degrade
//! silently on lock poisoning / channel disconnect (D6).
//! The seam differs only in that the DM controller also emits a typed projection;
//! BOTH use `register_snapshot_tick_observer` for their reconcile→apply path.
//! The DM projection closure is a PURE READ that never reconciles — keeping
//! side-effect and data-projection concerns on separate, independently-owned seams.
//!
//! Originally lived in `apps/chirp/crates/nmp-app-chirp/src/{dm,zap_receipts}_runtime.rs`.
//! Lifted here so any NMP-based app gets canonical DM + zap subscription
//! behaviour through one `register_defaults` call. The DM keys also emit typed
//! FlatBuffers sidecars (ADR-0037, Wave A): `nmp.nip17.dm_inbox` (`NDMI`) and
//! `nmp.nip17.dm_relay_list` (`NDRL`).

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::actor::{InterestsCommand, PublishCommand};
use nmp_core::substrate::{
    HostCapabilities, IdentityChangeRegistrar, IngestParserRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::{read_eligible_relay_urls, AppRelaySlot};
use nmp_nip17::{
    active_giftwrap_inbox_identity, active_giftwrap_inbox_interest, DmInboxProjection,
    DmRuntimeEffect, DmRuntimeState,
};
use nmp_nip57::{self_zap_receipts_identity, self_zap_receipts_interest};

// ───────────────────────────────────────────────────────────────────────
// NIP-17 DM runtime
// ───────────────────────────────────────────────────────────────────────

/// Wire the NIP-17 DM runtime into `app`.
///
/// Registers [`DmInboxProjection`] as an `IngestParser` for kind:1059
/// under slot `"nip17.dm_inbox"` + the `"nmp.nip17.dm_inbox"` snapshot
/// projection, then captures a `DmRuntimeController` that:
///
/// 1. Drives reconciliation (active-account gift-wrap inbox interest +
///    kind:10050 relay-list publishes) once per tick via a
///    **per-tick observer** — the same seam the zap-receipts runtime uses.
///    This is a pure side-effect path; it produces no projection data.
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
          + IngestParserRegistrar
          + SnapshotProjectionRegistrar),
) {
    register_inbox_projection(app);

    let controller = Arc::new(DmRuntimeController {
        relay_slot: app.configured_relays_handle(),
        // Pubkey-only identity (Finding C): the relay-list reconciler only
        // needs the active pubkey, never secret keys — read the slot the kernel
        // populates for every backend so bunker accounts reconcile their inbox
        // interest + kind:10050 relay list too.
        active_pubkey: app.active_pubkey(),
        tx: app.actor_sender(),
        state: Mutex::new(DmRuntimeState::default()),
    });

    // Per-tick reconciler: drives the active-account gift-wrap inbox interest +
    // kind:10050 relay-list publish side-effects once per tick. Produces no
    // projection data — this is the correct seam for pure side-effect work
    // (same pattern as `register_zap_receipts_runtime`). Reconciliation fires
    // independently of whether any host consumes the typed projection below.
    let controller_tick = Arc::clone(&controller);
    app.register_snapshot_tick_observer(move || controller_tick.tick());

    // Typed FlatBuffers sidecar (ADR-0037, Wave A): PURE READ — reads the
    // relay-list state and encodes it. Reconciliation (push/withdraw) is
    // handled exclusively by the tick observer above so this closure is
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
) {
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
}

impl DmRuntimeController {
    /// Per-tick reconciler — runs once per snapshot tick as a tick observer.
    ///
    /// Reads the current relay-list state, runs [`DmRuntimeState::reconcile`]
    /// against it, and applies any resulting effects (push/withdraw inbox
    /// interest, publish relay-list event). This is the ONLY path that emits
    /// actor commands from the DM relay-list runtime; the typed projection
    /// closure is a pure read.
    ///
    /// D6: a poisoned state mutex degrades to a no-op (no effects emitted,
    /// no crash on the actor thread). D8: channel send is non-blocking.
    pub(crate) fn tick(&self) {
        let relay_list = self.typed_relay_list();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for effect in state.reconcile(
            relay_list.active_pubkey.as_deref(),
            &relay_list.read_relay_urls,
        ) {
            self.apply(effect);
        }
    }

    /// Read the current `(active_pubkey, read_relay_urls)` into the SINGLE
    /// `nmp_nip17::DmRelayList` read model. A PURE READ — it deliberately does
    /// NOT run `reconcile`, so the push/withdraw actor traffic stays driven
    /// exclusively by [`Self::tick`] (exactly once per tick via the tick
    /// observer). The typed projection closure observes the same
    /// single-threaded actor tick, so it mirrors the reconciler's view
    /// field-for-field.
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
// NIP-57 zap-receipts runtime
// ───────────────────────────────────────────────────────────────────────

/// Wire the NIP-57 self-zap-receipts subscription runtime into `app`.
///
/// Registers a **per-tick observer** (`register_snapshot_tick_observer`) whose
/// body reconciles the active-account kind:9735 inbox interest against the
/// last-applied pubkey, emitting at most one ensure (on account change /
/// first sign-in) and at most one drop-owner (on logout / before the
/// re-ensure) per tick. It contributes NO snapshot data — it is a pure per-tick
/// reconciler, so it uses the generic tick-observer seam rather than the
/// projection registry (which it previously abused by returning a `Value::Null`
/// projection purely to obtain the per-tick callback).
///
/// Visible-card zap counts are acquired through the scoped
/// `nmp.nip01.visible_note_relations` action/interest path; the template ships
/// only this active-account receipt reconciler.
///
/// Called by [`super::register_defaults`]; exposed `pub` so an app crate
/// that opts out of the wholesale defaults can still wire just the zap
/// subscription by itself.
pub fn register_zap_receipts_runtime(app: &(impl HostCapabilities + SnapshotProjectionRegistrar)) {
    let controller = Arc::new(ZapReceiptsRuntimeController {
        // Pubkey-only identity (Finding C): the self-zap-receipts reconciler
        // only needs the active pubkey for the kind:9735 `#p` subscription —
        // never secret keys — so bunker accounts activate it too.
        active_pubkey: app.active_pubkey(),
        tx: app.actor_sender(),
        last_pushed_pubkey: Mutex::new(None),
    });
    app.register_snapshot_tick_observer(move || controller.tick());
}

/// Per-tick reconciler for the active-account zap-receipts interest.
struct ZapReceiptsRuntimeController {
    /// Pubkey-only identity slot (Finding C): the active account's hex pubkey,
    /// populated for every backend including bunker. Identity only — never
    /// secret key material.
    active_pubkey: nmp_core::slots::ActiveAccountSlot,
    tx: nmp_core::CommandSender,
    last_pushed_pubkey: Mutex<Option<String>>,
}

impl ZapReceiptsRuntimeController {
    /// Reconcile the active-account zap-receipts interest once per snapshot
    /// tick. Produces no snapshot data — it only diffs the active pubkey against
    /// the last-pushed one and enqueues scoped interest commands on change (D8:
    /// enqueue-only, non-blocking).
    fn tick(&self) {
        let active = self.active_pubkey();

        // D6 — a poisoned slot is silently treated as "no prior ensure" so
        // the next sign-in still ensures the interest.
        let mut last = self
            .last_pushed_pubkey
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match (active.as_deref(), last.as_deref()) {
            // No change — common case, fast path, no actor traffic.
            (Some(now), Some(prev)) if now == prev => {}
            // Sign-in (or first-ever push).
            (Some(now), None) => {
                let _ = self
                    .tx
                    .send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
                        identity: self_zap_receipts_identity(),
                        interest: self_zap_receipts_interest(now),
                    }));
                *last = Some(now.to_string());
            }
            // Account switch: drop old scoped owner, then ensure new shape.
            (Some(now), Some(_prev)) => {
                let _ = self.tx.send(ActorCommand::Interests(
                    InterestsCommand::DropInterestOwner(self_zap_receipts_identity()),
                ));
                let _ = self
                    .tx
                    .send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
                        identity: self_zap_receipts_identity(),
                        interest: self_zap_receipts_interest(now),
                    }));
                *last = Some(now.to_string());
            }
            // Logout: drop standing owner, clear slot.
            (None, Some(_)) => {
                let _ = self.tx.send(ActorCommand::Interests(
                    InterestsCommand::DropInterestOwner(self_zap_receipts_identity()),
                ));
                *last = None;
            }
            // Cold start before sign-in: nothing to do.
            (None, None) => {}
        }
    }

    fn active_pubkey(&self) -> Option<String> {
        // Identity straight from the pubkey slot — already hex, no keypair
        // derivation. `None` on a poisoned lock or no signed-in account.
        self.active_pubkey.lock().ok().and_then(|slot| slot.clone())
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
pub use bookmarks_runtime::register_bookmark_runtime;

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

// Co-located zap-reconciler unit tests live in a sibling file (kept out of this
// module body to hold it under the 300-LOC ceiling) but compile as a child
// module so they reach the private `ZapReceiptsRuntimeController`.
#[cfg(test)]
#[path = "runtimes_zap_tests.rs"]
mod zap_tests;

// Mute-list runtime controller unit tests — mirrors runtimes_zap_tests.rs but
// for the MuteRuntimeController (kind:10000 authors=[active_pubkey] interest).
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
