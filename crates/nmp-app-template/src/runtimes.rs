//! Canonical host-side runtime controllers wired by [`super::register_defaults`].
//!
//! Two snapshot-projection-driven reconcilers that own the active-account
//! per-tick `PushInterest` / `WithdrawInterest` book-keeping the kernel
//! itself cannot do (D0 — `nmp-core` ships no DM/zap nouns):
//!
//! 1. [`register_dm_runtime`] — NIP-17 DM inbox.
//!    * Wires the kind:1059 raw-event [`nmp_nip17::DmInboxProjection`] +
//!      its `"nmp.nip17.dm_inbox"` snapshot projection.
//!    * Owns a `DmRuntimeController` that on every snapshot tick
//!      reconciles the active-account gift-wrap inbox interest (kind:1059
//!      `#p`) and any pending kind:10050 publishes against the
//!      relay-edit-rows snapshot.
//! 2. [`register_zap_receipts_runtime`] — NIP-57 self-zap receipts.
//!    * Owns a `ZapReceiptsRuntimeController` that pushes /
//!      withdraws the active-account kind:9735 `#p` subscription on
//!      sign-in / account switch / sign-out (the `nmp.nip57.zaps`
//!      aggregate projection is registered separately by an app crate
//!      that wants the per-target counts; the template ships only the
//!      subscription reconciler).
//!
//! # Both controllers
//!
//! * Are captured by a `register_snapshot_projection` closure — the
//!   snapshot tick is what drives reconciliation. An event-observer
//!   would only fire on event arrival; the push has to happen
//!   *before* the first event (the moment the user signs in).
//! * Reconcile against a single `Mutex<Option<String>>` of the last-pushed
//!   pubkey. The withdraw side uses a pubkey-invariant interest id so
//!   an account switch cleanly replaces the prior subscription rather
//!   than leaking one per pubkey.
//! * Degrade silently on lock poisoning or actor-channel disconnect (D6).
//!
//! Originally lived in `apps/chirp/nmp-app-chirp/src/dm_runtime.rs` +
//! `zap_receipts_runtime.rs` (115 + 167 LOC). Lifted here so any
//! NMP-based app gets canonical DM + zap subscription behaviour through
//! one `register_defaults` call.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use nmp_core::{
    read_eligible_relay_urls, ActorCommand, RawEventObserver, RelayEditRowsSlot,
};
use nmp_ffi::NmpApp;
use nmp_nip17::{
    active_giftwrap_inbox_interest, active_giftwrap_inbox_interest_id, DmInboxProjection,
    DmRuntimeEffect, DmRuntimeState,
};
use nmp_nip57::{self_zap_receipts_interest, self_zap_receipts_interest_id};
use serde::Serialize;

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
pub fn register_dm_runtime(app: &NmpApp) {
    register_inbox_projection(app);

    let controller = Arc::new(DmRuntimeController {
        relay_rows: app.relay_edit_rows_handle(),
        local_keys: app.active_local_keys(),
        tx: app.actor_sender(),
        state: Mutex::new(DmRuntimeState::default()),
    });
    app.register_snapshot_projection("nmp.nip17.dm_relay_list", move || controller.snapshot_json());
}

fn register_inbox_projection(app: &NmpApp) {
    let projection = Arc::new(DmInboxProjection::new(app.active_local_keys()));
    let observer_id = app.register_raw_event_observer(
        DmInboxProjection::kind_filter(),
        Arc::clone(&projection) as Arc<dyn RawEventObserver>,
    );
    if observer_id.0 == 0 {
        return;
    }
    if let Some(prev) = app.swap_nip17_dm_inbox_observer(Some(observer_id)) {
        app.unregister_raw_event_observer(prev);
    }
    app.register_snapshot_projection("nmp.nip17.dm_inbox", move || projection.snapshot_json());
}

struct DmRuntimeController {
    relay_rows: RelayEditRowsSlot,
    local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    tx: Sender<ActorCommand>,
    state: Mutex<DmRuntimeState>,
}

impl DmRuntimeController {
    fn snapshot_json(&self) -> serde_json::Value {
        let active_pubkey = self.active_pubkey();
        let read_relay_urls = self.read_relay_urls();
        {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            for effect in state.reconcile(active_pubkey.as_deref(), &read_relay_urls) {
                self.apply(effect);
            }
        }
        serde_json::to_value(DmRelayListSnapshot {
            active_pubkey,
            read_relay_urls,
        })
        .unwrap_or_else(|_| serde_json::json!({ "active_pubkey": null, "read_relay_urls": [] }))
    }

    fn active_pubkey(&self) -> Option<String> {
        self.local_keys
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|keys| keys.public_key().to_hex()))
    }

    fn read_relay_urls(&self) -> Vec<String> {
        self.relay_rows
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
                }
            }
        };
        let _ = self.tx.send(cmd);
    }
}

#[derive(Serialize)]
struct DmRelayListSnapshot {
    active_pubkey: Option<String>,
    read_relay_urls: Vec<String>,
}

// ───────────────────────────────────────────────────────────────────────
// NIP-57 zap-receipts runtime
// ───────────────────────────────────────────────────────────────────────

/// Wire the NIP-57 self-zap-receipts subscription runtime into `app`.
///
/// Registers a snapshot-projection under `"nmp.nip57.zap_subscription"`
/// whose closure body reconciles the active-account kind:9735 inbox
/// interest against the last-applied pubkey, emitting at most one
/// `PushInterest` (on account change / first sign-in) and at most one
/// `WithdrawInterest` (on logout / before the re-push) per tick.
///
/// The per-target zap aggregate read (`"nmp.nip57.zaps"`, fed by
/// [`nmp_nip57::ZapsAggregateProjection`]) is registered separately by
/// the per-app crate that wants it — the template ships only the
/// subscription reconciler so apps that don't care about per-row zap
/// counts don't carry an unused observer.
///
/// Called by [`super::register_defaults`]; exposed `pub` so an app crate
/// that opts out of the wholesale defaults can still wire just the zap
/// subscription by itself.
pub fn register_zap_receipts_runtime(app: &NmpApp) {
    let controller = Arc::new(ZapReceiptsRuntimeController {
        local_keys: app.active_local_keys(),
        tx: app.actor_sender(),
        last_pushed_pubkey: Mutex::new(None),
    });
    app.register_snapshot_projection("nmp.nip57.zap_subscription", move || {
        controller.tick_and_snapshot()
    });
}

/// Per-tick reconciler for the active-account zap-receipts interest.
struct ZapReceiptsRuntimeController {
    local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    tx: Sender<ActorCommand>,
    last_pushed_pubkey: Mutex<Option<String>>,
}

impl ZapReceiptsRuntimeController {
    fn tick_and_snapshot(&self) -> serde_json::Value {
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
                let _ = self.tx.send(ActorCommand::PushInterest(
                    self_zap_receipts_interest(now),
                ));
                *last = Some(now.to_string());
            }
            // Account switch: withdraw old (by pubkey-invariant id), push new.
            (Some(now), Some(_prev)) => {
                let _ = self.tx.send(ActorCommand::WithdrawInterest(
                    self_zap_receipts_interest_id(),
                ));
                let _ = self.tx.send(ActorCommand::PushInterest(
                    self_zap_receipts_interest(now),
                ));
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

        serde_json::Value::Null
    }

    fn active_pubkey(&self) -> Option<String> {
        self.local_keys
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|keys| keys.public_key().to_hex()))
    }
}
