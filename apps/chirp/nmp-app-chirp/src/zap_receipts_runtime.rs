//! Chirp NIP-57 zap-receipts runtime wiring.
//!
//! Host-shell glue around the active-account kind:9735 subscription. The
//! kernel ships zero zap nouns; the subscription that feeds
//! [`nmp_nip57::ZapsAggregateProjection`] is pushed from here as a generic
//! [`nmp_core::planner::LogicalInterest`] (`nmp_nip57::self_zap_receipts_interest`)
//! exactly the way [`crate::dm_runtime`] pushes the NIP-17 gift-wrap inbox
//! interest.
//!
//! # Wiring
//!
//! Registered after [`crate::dm_runtime::register_dm_runtime`] inside the
//! Chirp app-init path ([`crate::ffi::register::nmp_app_chirp_register`]).
//! The controller is captured by a snapshot-projection closure under
//! `"nmp.nip57.zap_subscription"` — the closure's `serde_json::Value` return
//! is intentionally a marker (the kernel exposes
//! `"nmp.nip57.zaps"` for the per-target aggregate read), what makes it
//! load-bearing is the reconcile side-effect that runs before the
//! return. That cadence (one tick = one reconciliation) is the only
//! requirement; the snapshot tick already runs at the kernel's own pace.
//!
//! # Why a snapshot-projection closure (not `register_event_observer`)
//!
//! The DM-runtime controller in this app uses the same trick: snapshot
//! projections fire on every tick and have access to a shared `Arc`-held
//! controller. An event-observer-based controller would only fire when an
//! event arrived — but the push needs to happen *before* the first event,
//! the moment the user signs in. The snapshot tick is what schedules us.
//!
//! # State
//!
//! The reconciler is a single `Mutex<Option<String>>` — the pubkey the
//! inbox interest was last pushed for. The single-slot withdraw is keyed
//! on the pubkey-invariant
//! [`nmp_nip57::self_zap_receipts_interest_id`], so an account switch
//! cleanly replaces the prior subscription rather than accumulating one
//! per-pubkey forever.
//!
//! # D-doctrine
//!
//! * **D0** — the kernel learns nothing new. This file lives in the host
//!   app crate, not in `nmp-core`, because zap orchestration is a NIP-57
//!   concern (host shell + protocol crate) and `nmp-core` must stay
//!   protocol-agnostic.
//! * **D6** — every failure path (poisoned mutex, send failure on the
//!   actor channel, missing local-keys slot) is a silent no-op so a
//!   transient host hiccup never crashes the snapshot tick.
//! * **D8** — the reconcile body runs synchronously inside a snapshot
//!   tick; no background tasks, no polling.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use nmp_core::{ActorCommand, NmpApp};
use nmp_nip57::{self_zap_receipts_interest, self_zap_receipts_interest_id};

/// Wire the NIP-57 zap-receipts runtime into `app`.
///
/// Registers a snapshot-projection under `"nmp.nip57.zap_subscription"`
/// whose closure body reconciles the active-account kind:9735 inbox
/// interest against the last-applied pubkey, emitting at most one
/// `ActorCommand::PushInterest` (on account change / first sign-in) and at
/// most one `ActorCommand::WithdrawInterest` (on logout / before the
/// re-push) per tick.
///
/// MUST be called once, after [`crate::dm_runtime::register_dm_runtime`],
/// from the Chirp app-init path. Calling it twice would register two
/// snapshot-projection closures under the same key — only one survives, but
/// the orphan controller's `Arc` leaks (small, bounded — one per stray
/// call). The current call-site (`register::nmp_app_chirp_register`) is
/// the only one.
pub(crate) fn register_zap_receipts_runtime(app: &NmpApp) {
    let controller = Arc::new(ZapReceiptsRuntimeController {
        local_keys: app.nip17_local_keys(),
        tx: app.actor_sender(),
        last_pushed_pubkey: Mutex::new(None),
    });
    app.register_snapshot_projection("nmp.nip57.zap_subscription", move || {
        controller.tick_and_snapshot()
    });
}

/// Per-tick reconciler for the active-account zap-receipts interest.
///
/// Shape mirrors [`crate::dm_runtime::DmRuntimeController`] line for
/// line; the only intentional differences are:
/// - No DM-relay-list publish path (zap receipts are public content; no
///   per-recipient relay-list publish exists for them).
/// - No [`nmp_core::RelayEditRowsSlot`] — the subscription target is the
///   active account itself, not a host-configured relay set.
struct ZapReceiptsRuntimeController {
    /// Shared local-keys slot the kernel writes on every identity mutation
    /// (`NmpApp::nip17_local_keys`). We read it on every tick to learn the
    /// active pubkey; `None` (not signed in) → withdraw + clear.
    local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    /// Actor command channel — the runtime translates pubkey diffs into
    /// one `Push` / `Withdraw` per tick.
    tx: Sender<ActorCommand>,
    /// Pubkey the inbox interest was last pushed for. Diffed against the
    /// active account on each tick so we emit at most one Push / one
    /// Withdraw per change.
    last_pushed_pubkey: Mutex<Option<String>>,
}

impl ZapReceiptsRuntimeController {
    /// Snapshot tick entry point. Reconciles the active pubkey against the
    /// last-applied pubkey, sends any required `ActorCommand`s, and
    /// returns `Value::Null` (the snapshot key exists so the kernel ticks
    /// us — the payload is intentionally empty; the per-target aggregate
    /// read is at `"nmp.nip57.zaps"`).
    fn tick_and_snapshot(&self) -> serde_json::Value {
        let active = self.active_pubkey();

        // D6 — a poisoned `last_pushed_pubkey` slot is silently treated as
        // "no prior push" so the next sign-in still pushes the interest.
        let mut last = self
            .last_pushed_pubkey
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match (active.as_deref(), last.as_deref()) {
            // No change — common case, fast path, no actor traffic.
            (Some(now), Some(prev)) if now == prev => {}
            // Sign-in (or first-ever push): push fresh interest, record
            // pubkey.
            (Some(now), None) => {
                let _ = self.tx.send(ActorCommand::PushInterest(
                    self_zap_receipts_interest(now),
                ));
                *last = Some(now.to_string());
            }
            // Account switch: withdraw old by id, push new for the new
            // pubkey, record. The id is pubkey-invariant
            // (`self_zap_receipts_interest_id`) so withdraw-by-id
            // matches the prior push regardless of the prior pubkey.
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

    /// Read the active account pubkey (hex) from the shared local-keys slot.
    /// `None` (not signed in or poisoned slot) → no active account.
    fn active_pubkey(&self) -> Option<String> {
        self.local_keys
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|keys| keys.public_key().to_hex()))
    }
}
