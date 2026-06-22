//! NIP-51 search-relay-list runtime — wires the kind:10007
//! [`SearchRelayListProjection`] into an [`AppHost`] (observer + per-tick
//! interest reconciler via [`SearchRelayRuntimeController`]).
//!
//! The `register_search_relay_runtime` entry point is re-exported from
//! `runtimes` and from `nmp_defaults` so callers reach it at
//! `runtimes::register_search_relay_runtime` /
//! `nmp_defaults::register_search_relay_runtime`.
//!
//! # What callers get
//!
//! `register_search_relay_runtime` returns `Arc<SearchRelayListProjection>`.
//! Pass it to [`crate::search_defaults::effective_search_relays`] to get
//! the effective relay list (user's kind:10007 list, else the app-default
//! fallback). A higher-order NIP-50 search crate that needs to open a relay
//! subscription on the right relays calls that helper rather than reaching
//! into the projection directly.
//!
//! [`AppHost`]: nmp_core::substrate::AppHost

use std::sync::{Arc, Mutex};

use nmp_core::substrate::{EventObserverRegistrar, HostCapabilities, SnapshotProjectionRegistrar};
use nmp_core::{ActorCommand, KernelEventObserver};
use nmp_nip51::{
    active_search_relay_list_interest, active_search_relay_list_interest_id,
    SearchRelayListProjection,
};

/// Wire the NIP-51 search-relay-list observer into `app` and return the
/// [`SearchRelayListProjection`] so callers can read the active account's
/// kind:10007 relay list.
///
/// # What this function does
///
/// 1. **Pubkey slot bridge** — hands [`SearchRelayListProjection`] the shared
///    `AppHost::active_pubkey()` hex slot (populated by the kernel for EVERY
///    backend including bunker). The projection reads it at event-ingest time
///    and at query time, so it is always consistent with the active account.
/// 2. **Ingest observer** — registers `SearchRelayListProjection` as a
///    [`KernelEventObserver`] FIRST, so the kernel fan-out delivers kind:10007
///    events to the projection. The projection filters for the active account's
///    author (account-switch safety enforced at read time by the
///    owner-pubkey gate inside the projection).
/// 3. **Tick observer — [`SearchRelayRuntimeController`]** — registered LAST
///    (ordering contract: observer BEFORE tick observer). On every snapshot
///    tick reconciles the active pubkey against the last-pushed one, emitting
///    `PushInterest` / `WithdrawInterest` to the kernel so the search-relay
///    interest (kind:10007, authors=[active]) is always live for the signed-in
///    account.
/// 4. **Returns the `Arc<SearchRelayListProjection>`** — the caller passes it
///    to [`crate::search_defaults::effective_search_relays`] to resolve the
///    effective search relay set (user list, else app default).
///
/// # Ordering contract
///
/// The event observer MUST be registered before the tick observer. The tick
/// observer pushes the search-relay interest on its first call, which may
/// trigger a synchronous cache-serve drain. If the event observer is not
/// registered yet at that point, the drain delivers events to nobody. Register
/// in this order:
/// 1. `app.register_event_observer(...)` — FIRST
/// 2. `app.register_snapshot_tick_observer(...)` — LAST
///
/// # Account-switch safety
///
/// [`SearchRelayListProjection`] is self-contained: the read path re-reads the
/// live `active_pubkey` slot on every call and gates against the
/// `owner_pubkey` stored inside the `SearchRelaySet`. If the active account
/// changed between the last kind:10007 ingest and the read, methods return a
/// default empty list — stale data from the prior account is invisible.
/// [`SearchRelayRuntimeController`] additionally withdraws the prior interest
/// and pushes a fresh one on account switch so no stale subscription persists
/// in the planner.
///
/// # D0 hygiene
///
/// This function names `kind:10007` only as a numeric literal inside
/// `nmp-nip51`. The term "search relays" enters `nmp-core` nowhere:
/// the composition crate (here) is entitled to name NIP constants directly
/// per ADR-0046.
///
/// Called by [`crate::register_defaults`]; exposed `pub` so an app crate that
/// opts out of the wholesale defaults can still wire just the search-relay
/// runtime by itself.
pub fn register_search_relay_runtime(
    app: &(impl EventObserverRegistrar + HostCapabilities + SnapshotProjectionRegistrar),
) -> Arc<SearchRelayListProjection> {
    // ── 1. Active-pubkey slot ────────────────────────────────────────────────
    let projection = Arc::new(SearchRelayListProjection::new(app.active_pubkey()));

    // ── 2. Register as ingest observer — FIRST (ordering contract) ──────────
    //
    // Must be registered BEFORE the tick observer below. The tick observer
    // pushes the search-relay interest on its first call, which triggers a
    // synchronous cache-serve drain. If this observer is not registered yet at
    // that point, the drain delivers kind:10007 events to nobody.
    app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);

    // ── 3. Per-tick reconciler — LAST (ordering contract) ────────────────────
    //
    // `SearchRelayRuntimeController` owns the active-account kind:10007 interest
    // slot. On sign-in it pushes `active_search_relay_list_interest(pubkey)` so
    // the kernel has a live `authors=[active_pubkey] / kinds=[10007]`
    // subscription. On account switch it withdraws the old interest (by
    // pubkey-invariant id) and pushes a new one. On sign-out it withdraws.
    // Mirrors the `MuteRuntimeController` pattern.
    let controller = Arc::new(SearchRelayRuntimeController {
        active_pubkey: app.active_pubkey(),
        tx: app.actor_sender(),
        last_pushed_pubkey: Mutex::new(None),
    });
    let controller_tick = Arc::clone(&controller);
    app.register_snapshot_tick_observer(move || controller_tick.tick());

    projection
}

/// Per-tick reconciler for the active-account search-relay-list interest.
///
/// Owns the kind:10007 `authors=[active_pubkey]` interest slot. On every
/// snapshot tick diffs the active pubkey against the last-pushed one and
/// enqueues `PushInterest` / `WithdrawInterest` on change (D8: non-blocking).
///
/// Exposed `pub(crate)` so the unit tests in `runtimes_search_relay_tests` can
/// construct a controller without a real `AppHost`.
pub(crate) struct SearchRelayRuntimeController {
    /// Pubkey-only identity slot (Finding C): the active account's hex pubkey,
    /// populated for every backend including bunker. Identity only — never
    /// secret key material.
    pub(crate) active_pubkey: nmp_core::slots::ActiveAccountSlot,
    pub(crate) tx: nmp_core::CommandSender,
    pub(crate) last_pushed_pubkey: Mutex<Option<String>>,
}

impl SearchRelayRuntimeController {
    /// Reconcile the active-account search-relay-list interest once per
    /// snapshot tick.
    ///
    /// Diffs the active pubkey against the last-pushed one and enqueues
    /// `PushInterest` / `WithdrawInterest` on change. D8: channel send is
    /// non-blocking; D6: a poisoned last-pushed mutex degrades to "no prior
    /// push" so the next sign-in still pushes.
    pub(crate) fn tick(&self) {
        let active = self.active_pubkey();

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
                    active_search_relay_list_interest(now),
                ));
                *last = Some(now.to_string());
            }
            // Account switch: withdraw old (by pubkey-invariant id), push new.
            (Some(now), Some(_prev)) => {
                let _ = self.tx.send(ActorCommand::WithdrawInterest(
                    active_search_relay_list_interest_id(),
                ));
                let _ = self.tx.send(ActorCommand::PushInterest(
                    active_search_relay_list_interest(now),
                ));
                *last = Some(now.to_string());
            }
            // Logout: withdraw standing interest, clear slot.
            (None, Some(_)) => {
                let _ = self.tx.send(ActorCommand::WithdrawInterest(
                    active_search_relay_list_interest_id(),
                ));
                *last = None;
            }
            // Cold start before sign-in: nothing to do.
            (None, None) => {}
        }
    }

    fn active_pubkey(&self) -> Option<String> {
        self.active_pubkey
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }
}

// Co-located search-relay reconciler unit tests live in a sibling file (kept
// out of this module body to hold it under the 300-LOC ceiling) but compile as
// a child module so they reach the private `SearchRelayRuntimeController`.
#[cfg(test)]
#[path = "runtimes_search_relay_tests.rs"]
mod search_relay_tests;
