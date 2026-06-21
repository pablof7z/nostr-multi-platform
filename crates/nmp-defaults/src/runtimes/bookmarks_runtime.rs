//! NIP-51 bookmark-list runtime wiring.
//!
//! This composition helper installs one shared [`BookmarkListProjection`] as
//! the kind:10003 observer and read-modify-write state backing the default
//! add/remove bookmark actions. It also owns a [`BookmarksRuntimeController`]
//! registered via the generic **per-tick observer** seam
//! (`register_snapshot_tick_observer`) that pushes / withdraws the
//! active-account kind:10003 `authors=[pubkey]` subscription on sign-in /
//! account switch / sign-out — mirroring the `ZapReceiptsRuntimeController`
//! pattern in `runtimes.rs`.
//!
//! # Ordering contract
//!
//! The projection observer is registered BEFORE the first tick so a
//! synchronous cache-serve drain (cold-start) reaches it. The interest push
//! happens on the first tick, which fires after registration — there is no
//! gap window because the observer is already live.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ActionRegistrar, EventObserverRegistrar, HostCapabilities, SnapshotProjectionRegistrar};
use nmp_core::{ActorCommand, KernelEventObserver};
use nmp_nip51::{
    active_bookmark_list_interest, active_bookmark_list_interest_id, BookmarkListProjection,
};

/// Wire active-account kind:10003 bookmark projection and safe write actions,
/// and register the per-tick interest reconciler.
///
/// 1. Creates one [`BookmarkListProjection`] shared across the observer and the
///    action modules.
/// 2. Registers the projection as a [`KernelEventObserver`] BEFORE the first
///    tick so cold-start cache-serve events reach it.
/// 3. Registers the add/remove bookmark action modules (read-modify-write
///    against the same projection).
/// 4. Creates a [`BookmarksRuntimeController`] and registers it as a per-tick
///    observer — it pushes the `active_bookmark_list_interest` on sign-in,
///    withdraws-then-pushes on account switch, and withdraws on logout.
pub fn register_bookmark_runtime(
    app: &mut (impl ActionRegistrar
              + EventObserverRegistrar
              + HostCapabilities
              + SnapshotProjectionRegistrar),
) -> Arc<BookmarkListProjection> {
    // ── 1 + 2. Projection + observer ─────────────────────────────────────
    //
    // Register the observer BEFORE the first tick (ordering contract above).
    let projection = Arc::new(BookmarkListProjection::new(app.active_pubkey()));
    app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);

    // ── 3. Action modules ─────────────────────────────────────────────────
    nmp_nip51::register_bookmark_actions(app, Arc::clone(&projection));

    // ── 4. Per-tick interest reconciler ──────────────────────────────────
    //
    // Mirrors `ZapReceiptsRuntimeController` (runtimes.rs): a single
    // `Mutex<Option<String>>` tracks the last-pushed pubkey; the tick method
    // diffs against the current active pubkey and emits at most one
    // Push/Withdraw pair per tick.
    let controller = Arc::new(BookmarksRuntimeController {
        active_pubkey: app.active_pubkey(),
        tx: app.actor_sender(),
        last_pushed_pubkey: Mutex::new(None),
    });
    app.register_snapshot_tick_observer(move || controller.tick());

    projection
}

/// Per-tick reconciler for the active-account bookmark-list interest.
///
/// Owns a pubkey-invariant interest id slot so an account switch cleanly
/// replaces the prior subscription rather than leaking it. Exposed
/// `pub(crate)` so the sibling test module can construct one without a real
/// `AppHost`.
pub(crate) struct BookmarksRuntimeController {
    /// Pubkey-only identity slot (Finding C): the active account's hex pubkey,
    /// populated for every backend including bunker. Identity only — never
    /// secret key material.
    active_pubkey: nmp_core::slots::ActiveAccountSlot,
    tx: nmp_core::CommandSender,
    last_pushed_pubkey: Mutex<Option<String>>,
}

impl BookmarksRuntimeController {
    /// Reconcile the active-account bookmark-list interest once per snapshot
    /// tick. Produces no snapshot data — it only diffs the active pubkey
    /// against the last-pushed one and enqueues Push/Withdraw on change (D8:
    /// enqueue-only, non-blocking).
    pub(crate) fn tick(&self) {
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
                    .send(ActorCommand::PushInterest(active_bookmark_list_interest(now)));
                *last = Some(now.to_string());
            }
            // Account switch: withdraw old (by pubkey-invariant id), push new.
            (Some(now), Some(_prev)) => {
                let _ = self.tx.send(ActorCommand::WithdrawInterest(
                    active_bookmark_list_interest_id(),
                ));
                let _ = self
                    .tx
                    .send(ActorCommand::PushInterest(active_bookmark_list_interest(now)));
                *last = Some(now.to_string());
            }
            // Logout: withdraw standing interest, clear slot.
            (None, Some(_)) => {
                let _ = self.tx.send(ActorCommand::WithdrawInterest(
                    active_bookmark_list_interest_id(),
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
        self.active_pubkey
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }
}

// Co-located bookmark reconciler unit tests live in a sibling file (kept out
// of this module body to hold it under the 300-LOC ceiling) but compile as a
// child module so they reach the private `BookmarksRuntimeController`.
#[cfg(test)]
#[path = "runtimes_bookmarks_tests.rs"]
mod bookmarks_tests;
