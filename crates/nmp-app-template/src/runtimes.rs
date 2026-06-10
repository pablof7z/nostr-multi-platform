//! Canonical host-side runtime controllers wired by [`super::register_defaults`].
//!
//! Two per-tick reconcilers that own the active-account `PushInterest` /
//! `WithdrawInterest` book-keeping the kernel itself cannot do (D0 — `nmp-core`
//! ships no DM/zap nouns):
//!
//! 1. [`register_dm_runtime`] — NIP-17 DM inbox.
//!    * Wires the kind:1059 raw-event [`nmp_nip17::DmInboxProjection`] +
//!      its `"nmp.nip17.dm_inbox"` snapshot projection.
//!    * Owns a `DmRuntimeController` whose `"nmp.nip17.dm_relay_list"`
//!      projection closure both reconciles (active-account gift-wrap inbox
//!      interest + pending kind:10050 publishes) AND emits the relay-list
//!      projection on every snapshot tick.
//! 2. [`register_zap_receipts_runtime`] — NIP-57 self-zap receipts.
//!    * Owns a `ZapReceiptsRuntimeController` registered via the generic
//!      **per-tick observer** seam (`register_snapshot_tick_observer`): it
//!      pushes / withdraws the active-account kind:9735 `#p` subscription on
//!      sign-in / account switch / sign-out and contributes NO snapshot data
//!      (the `nmp.nip57.zaps` aggregate projection is registered separately by
//!      an app crate that wants the per-target counts; the template ships only
//!      the subscription reconciler).
//!
//! # Both controllers
//!
//! The snapshot tick drives reconciliation — the push must happen *before* the
//! first event, the moment the user signs in. Both reconcile against a single
//! `Mutex<Option<String>>` of the last-pushed pubkey, withdrawing by a
//! pubkey-invariant interest id so an account switch cleanly replaces rather
//! than leaks, and degrade silently on lock poisoning / channel disconnect (D6).
//! The seam differs: the DM controller also emits a projection, so it reconciles
//! inside its projection closure; the zap controller emits nothing, so it uses
//! the data-free `register_snapshot_tick_observer` seam rather than abusing the
//! projection registry with a `Value::Null` projection.
//!
//! Originally lived in `apps/chirp/nmp-app-chirp/src/{dm,zap_receipts}_runtime.rs`.
//! Lifted here so any NMP-based app gets canonical DM + zap subscription
//! behaviour through one `register_defaults` call. The DM keys also emit typed
//! FlatBuffers sidecars (ADR-0037, Wave A): `nmp.nip17.dm_inbox` (`NDMI`) and
//! `nmp.nip17.dm_relay_list` (`NDRL`).

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::AppHost;
use nmp_core::{read_eligible_relay_urls, ActorCommand, AppRelaySlot, RawEventObserver};
use nmp_nip17::{
    active_giftwrap_inbox_interest, active_giftwrap_inbox_interest_id, DmInboxProjection,
    DmRuntimeEffect, DmRuntimeState,
};
use nmp_nip57::{self_zap_receipts_interest, self_zap_receipts_interest_id};

// ───────────────────────────────────────────────────────────────────────
// NIP-17 DM runtime
// ───────────────────────────────────────────────────────────────────────

/// Wire the NIP-17 DM runtime into `app`.
///
/// Registers the kind:1059 raw-event [`DmInboxProjection`] + the
/// `"nmp.nip17.dm_inbox"` snapshot projection, then captures a
/// `DmRuntimeController` under `"nmp.nip17.dm_relay_list"` whose closure
/// body reconciles the active-account gift-wrap inbox interest +
/// kind:10050 relay-list publishes against the relay-edit-rows snapshot
/// on every tick.
///
/// Called by [`super::register_defaults`]; exposed `pub` so an app crate
/// that opts out of the wholesale defaults can still wire just the DM
/// runtime by itself.
pub fn register_dm_runtime(app: &impl AppHost) {
    register_inbox_projection(app);

    let controller = Arc::new(DmRuntimeController {
        relay_slot: app.configured_relays_handle(),
        local_keys: app.active_local_keys(),
        tx: app.actor_sender(),
        state: Mutex::new(DmRuntimeState::default()),
    });

    // Typed FlatBuffers sidecar (ADR-0037, Wave A) ALONGSIDE the generic `Value`
    // projection (additive). The typed closure is a PURE READ — reconcile (which
    // emits actor commands) stays exclusively in the `snapshot_json` closure
    // below so the push/withdraw book-keeping runs exactly once per tick.
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
        })
    });

    app.register_snapshot_projection("nmp.nip17.dm_relay_list", move || {
        controller.snapshot_json()
    });
}

fn register_inbox_projection(app: &impl AppHost) {
    let projection = Arc::new(DmInboxProjection::new(app.active_local_keys()));
    let observer_id = app.register_raw_event_observer(
        DmInboxProjection::kind_filter(),
        Arc::clone(&projection) as Arc<dyn RawEventObserver>,
    );
    if observer_id.0 == 0 {
        return;
    }
    if let Some(prev) = app.swap_dm_inbox_observer(Some(observer_id)) {
        app.unregister_raw_event_observer(prev);
    }

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
        })
    });

    app.register_snapshot_projection("nmp.nip17.dm_inbox", move || projection.snapshot_json());
}

struct DmRuntimeController {
    relay_slot: AppRelaySlot,
    local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    tx: Sender<ActorCommand>,
    state: Mutex<DmRuntimeState>,
}

impl DmRuntimeController {
    fn snapshot_json(&self) -> serde_json::Value {
        let relay_list = self.typed_relay_list();
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for effect in
                state.reconcile(relay_list.active_pubkey.as_deref(), &relay_list.read_relay_urls)
            {
                self.apply(effect);
            }
        }
        serde_json::to_value(&relay_list)
            .unwrap_or_else(|_| serde_json::json!({ "active_pubkey": null, "read_relay_urls": [] }))
    }

    /// Read the current `(active_pubkey, read_relay_urls)` into the SINGLE
    /// `nmp_nip17::DmRelayList` read model both wire forms share. A PURE READ —
    /// it deliberately does NOT run `reconcile`, so the push/withdraw actor
    /// traffic stays driven exclusively by [`Self::snapshot_json`] (exactly once
    /// per tick). Both closures observe the same single-threaded actor tick, so
    /// the typed sidecar payload mirrors the JSON payload field-for-field.
    fn typed_relay_list(&self) -> nmp_nip17::DmRelayList {
        nmp_nip17::DmRelayList {
            active_pubkey: self.active_pubkey(),
            read_relay_urls: self.read_relay_urls(),
        }
    }

    fn active_pubkey(&self) -> Option<String> {
        self.local_keys
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|keys| keys.public_key().to_hex()))
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
                ActorCommand::PushInterest(active_giftwrap_inbox_interest(&pubkey))
            }
            DmRuntimeEffect::WithdrawInboxInterest => {
                ActorCommand::WithdrawInterest(active_giftwrap_inbox_interest_id())
            }
            DmRuntimeEffect::PublishRelayList { event, .. } => {
                // Non-dispatch internal path — the action-seam variant at
                // `nmp_nip17::PublishDmRelayListAction::execute` is where
                // the dispatch-side correlation_id round-trip happens.
                ActorCommand::PublishUnsignedEvent {
                    event,
                    correlation_id: None,
                    // Internal DM-relay-list publish signs with the active account.
                    signer_pubkey: None,
                }
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
/// last-applied pubkey, emitting at most one `PushInterest` (on account change /
/// first sign-in) and at most one `WithdrawInterest` (on logout / before the
/// re-push) per tick. It contributes NO snapshot data — it is a pure per-tick
/// reconciler, so it uses the generic tick-observer seam rather than the
/// projection registry (which it previously abused by returning a `Value::Null`
/// projection purely to obtain the per-tick callback).
///
/// The per-target zap aggregate read (`"nmp.nip57.zaps"`, fed by
/// [`nmp_nip57::ZapsAggregateProjection`]) is registered separately by the
/// per-app crate that wants it; the template ships only this reconciler.
///
/// Called by [`super::register_defaults`]; exposed `pub` so an app crate
/// that opts out of the wholesale defaults can still wire just the zap
/// subscription by itself.
pub fn register_zap_receipts_runtime(app: &impl AppHost) {
    let controller = Arc::new(ZapReceiptsRuntimeController {
        local_keys: app.active_local_keys(),
        tx: app.actor_sender(),
        last_pushed_pubkey: Mutex::new(None),
    });
    app.register_snapshot_tick_observer(move || controller.tick());
}

/// Per-tick reconciler for the active-account zap-receipts interest.
struct ZapReceiptsRuntimeController {
    local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    tx: Sender<ActorCommand>,
    last_pushed_pubkey: Mutex<Option<String>>,
}

impl ZapReceiptsRuntimeController {
    /// Reconcile the active-account zap-receipts interest once per snapshot
    /// tick. Produces no snapshot data — it only diffs the active pubkey against
    /// the last-pushed one and enqueues Push/Withdraw interest on change (D8:
    /// enqueue-only, non-blocking).
    fn tick(&self) {
        let active = self.active_pubkey();

        // D6 — a poisoned slot is silently treated as "no prior push" so
        // the next sign-in still pushes the interest.
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
                    .send(ActorCommand::PushInterest(self_zap_receipts_interest(now)));
                *last = Some(now.to_string());
            }
            // Account switch: withdraw old (by pubkey-invariant id), push new.
            (Some(now), Some(_prev)) => {
                let _ = self.tx.send(ActorCommand::WithdrawInterest(
                    self_zap_receipts_interest_id(),
                ));
                let _ = self
                    .tx
                    .send(ActorCommand::PushInterest(self_zap_receipts_interest(now)));
                *last = Some(now.to_string());
            }
            // Logout: withdraw standing interest, clear slot.
            (None, Some(_)) => {
                let _ = self.tx.send(ActorCommand::WithdrawInterest(
                    self_zap_receipts_interest_id(),
                ));
                *last = None;
            }
            // Cold start before sign-in: nothing to do.
            (None, None) => {}
        }
    }

    fn active_pubkey(&self) -> Option<String> {
        self.local_keys
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|keys| keys.public_key().to_hex()))
    }
}

// Co-located zap-reconciler unit tests live in a sibling file (kept out of this
// module body to hold it under the 300-LOC ceiling) but compile as a child
// module so they reach the private `ZapReceiptsRuntimeController`.
#[cfg(test)]
#[path = "runtimes_zap_tests.rs"]
mod zap_tests;
