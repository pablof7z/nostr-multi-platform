//! `ActiveFollowSet` — observable snapshot of the active account's follow set.
//!
//! # Overview
//!
//! The OP-centric home feed (V-59) needs to know, for any pubkey, whether the
//! active account follows that pubkey. The generic `RootIndexedFeed` engine in
//! `nmp-feed` (rung 3) consumes that knowledge as a closure predicate
//! (`Arc<dyn Fn(&str) -> bool + Send + Sync>`) — **not** a trait. v4 of the
//! design (see `docs/perf/op-centric-feed-architecture.md` §3-D) deleted the
//! `FollowSetLookup` trait and the `LogicalInterest::SocialTimeline` planner
//! variant in favour of a closure produced here and wired at the composition
//! root (`nmp-app-template`, rung 6). The rationale is recorded in
//! [ADR-0036](../../docs/decisions/0036-composition-root-followset-expansion.md).
//!
//! `ActiveFollowSet` is the **producer** of that closure. It owns an
//! `Arc<RwLock<BTreeSet<String>>>` of raw hex pubkeys for the active account's
//! follows (plus the active account's own pubkey, mirroring the kernel's
//! `timeline_authors` seeding — see below), keeps it current by observing
//! kind:3 ingest, and hands out:
//!
//! * [`ActiveFollowSet::follows`] — a sorted `Vec<String>` snapshot read.
//! * [`ActiveFollowSet::predicate`] — a closure that captures a clone of the
//!   internal `Arc<RwLock<…>>`, so a predicate handed out *before* a kind:3
//!   update reflects the update *live* (the closure-only design's load-bearing
//!   property — verified by the `predicate_reflects_live_updates` test).
//! * [`ActiveFollowSet::on_change`] — register a callback that fires on every
//!   set change (kind:3 update, account switch, logout).
//!
//! This rung lands the producer **unwired**: no consumer yet. Rung 5
//! (`nmp-nip01` instance) and rung 6 (`nmp-app-template` composition) consume
//! it.
//!
//! # Why no `&NmpApp` constructor
//!
//! The design doc sketches `ActiveFollowSet::new(app: &NmpApp)`. That is
//! pseudocode: `NmpApp` lives in `nmp-ffi`, which `nmp-nip02` depends on only
//! as a *dev*-dependency. A production `&NmpApp` parameter would invert the
//! dependency graph (`nmp-nip02 → nmp-ffi`). The substrate-clean realization —
//! mirroring the sibling [`crate::projection::FollowListProjection`] — is to
//! take the [`ActiveAccountSlot`] (re-exported through `nmp_core::slots`)
//! directly. The composition root registers this struct as a
//! `KernelEventObserver` separately, exactly as it already does for
//! `FollowListProjection`. No new crate edge in either direction (verified:
//! `cargo tree -p nmp-nip02` carries `nmp-core`, `nostr`, `serde`,
//! `serde_json` only — no `nmp-feed`, no `nmp-ffi`).
//!
//! # Self-inclusion
//!
//! `crates/nmp-core/src/kernel/ingest/contacts.rs::sync_follow_feed_interests`
//! seeds the active account's *own* pubkey into `timeline_authors` (lines
//! 162-164: `authors.insert(me.clone())`) so the user's own notes appear in
//! their home stream. `ActiveFollowSet` mirrors that inclusion: the active
//! account's own pubkey is always a member of the set (even before any kind:3
//! has arrived), so the producer agrees with the kernel's own follow-derived
//! authorship set.
//!
//! # Account switch / logout
//!
//! [`ActiveAccountSlot`] is `Arc<Mutex<Option<String>>>` — plain shared state
//! the kernel actor writes on account switch / logout. It carries **no** push
//! notification (no condvar, no channel), and neither `AppHost` nor `NmpApp`
//! exposes an observer for it. The explicit seam is
//! [`ActiveFollowSet::notify_account_changed`]: the composition root calls it
//! when the active account changes (rung 6 wires this to the same identity-
//! change path every other subsystem already uses). It re-reads the slot,
//! rebuilds the set for the new active account (clearing it entirely on
//! logout, when the slot is `None`), and fires `on_change`. A kind:3 ingest
//! does not cover logout — there is no logout-triggered kind:3 — so the
//! explicit seam is required for correctness, not convenience.
//!
//! # Host-declared follow-feed kinds
//!
//! `fix(nmp-core): keep follow-feed kinds host-declared` (commit `2f06cc66`)
//! made the *follow-feed subscription* REQ kinds host-declared. That change
//! touches which kinds the contact-list-authors REQ carries; it does **not**
//! touch the kind:3 ingest fan-out that `ActiveFollowSet` observes. The
//! sibling `FollowListProjection` (untouched by `2f06cc66`) is the living
//! proof: kind:3 events still fan out to `KernelEventObserver`s gated purely on
//! `event.kind == 3` and author == active, regardless of the host-declared
//! follow-feed kind set.
//!
//! # D-doctrine
//!
//! * **D0** — `nmp-nip02` is a NIP crate, so NIP-02 nouns are fine here. No
//!   NIP token leaks into `nmp-core`. The predicate is a std closure; no
//!   `nmp-feed` type appears in this crate's surface.
//! * **D5** — the set is bounded by the kernel's `TIMELINE_AUTHOR_LIMIT`
//!   applied upstream in `ingest_contacts` (the observer only ever sees an
//!   already-capped follow list); the `BTreeSet` here mirrors that capped set.
//! * **D6** — poisoned locks and a `None` active account degrade to an empty
//!   set / a `false` predicate, never a panic.
//! * **D8** — `on_kernel_event` does bounded work (one kind check, one lock,
//!   one `p`-tag scan, one set rebuild) on the actor thread between relay
//!   frames. No I/O, no blocking, no polling.
//! * **Raw data** — the set holds raw hex pubkeys only; display formatting is
//!   a higher-layer concern (2026-05-25 display-separation doctrine).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, RwLock};

use nmp_core::kinds::KIND_CONTACT_LIST;
use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;

/// A registered change callback. Fires on every follow-set transition
/// (kind:3 update, account switch, logout).
type ChangeCallback = Box<dyn Fn() + Send + Sync>;

/// Observable snapshot of the active account's follow set, as raw hex pubkeys.
///
/// Construct with [`ActiveFollowSet::new`] passing the kernel's
/// [`ActiveAccountSlot`] (clone of `Kernel::active_account_handle()`). The
/// composition root registers the returned `Arc<Self>` as a
/// [`KernelEventObserver`] so kind:3 events are ingested, and calls
/// [`ActiveFollowSet::notify_account_changed`] on identity change.
///
/// All state is `Arc`-internal so the struct is shared as `Arc<Self>` between
/// the observer registry, the composition root, and any handed-out predicate.
pub struct ActiveFollowSet {
    /// The active account's hex pubkey slot, written by the kernel actor on
    /// account switch / logout. `None` means no signed-in account → empty set,
    /// `false` predicate.
    active_pubkey: ActiveAccountSlot,
    /// The active account's follow set: raw hex pubkeys plus the active
    /// account's own pubkey (self-inclusion, mirroring `timeline_authors`).
    /// Captured (as an `Arc` clone) by every predicate handed out, so the
    /// predicate observes updates live.
    follows: Arc<RwLock<BTreeSet<String>>>,
    /// Registered change callbacks, fired on every set transition.
    on_change: Mutex<Vec<ChangeCallback>>,
}

impl ActiveFollowSet {
    /// Construct an `ActiveFollowSet` over the kernel's active-account slot.
    ///
    /// Returns `Arc<Self>` because the same value is shared three ways: as a
    /// [`KernelEventObserver`] in the kernel's observer registry, as the
    /// owner of the `on_change` registry the composition root drives, and as
    /// the source of the captured `Arc<RwLock<…>>` inside every handed-out
    /// predicate.
    ///
    /// The set is seeded immediately from the slot's current value (so a
    /// predicate handed out before any kind:3 arrives still returns `true` for
    /// the active account's own pubkey).
    #[must_use]
    pub fn new(active_pubkey: ActiveAccountSlot) -> Arc<Self> {
        let this = Arc::new(Self {
            active_pubkey,
            follows: Arc::new(RwLock::new(BTreeSet::new())),
            on_change: Mutex::new(Vec::new()),
        });
        // Seed self-inclusion from the slot's current active account (if any).
        // Does not fire `on_change` — there are no callbacks at construction.
        this.seed_self();
        this
    }

    /// Snapshot of the active account's follows as a sorted `Vec` of raw hex
    /// pubkeys (the active account's own pubkey is included — self-inclusion).
    ///
    /// Returns an empty `Vec` when no account is signed in or the lock is
    /// poisoned (D6).
    #[must_use]
    pub fn follows(&self) -> Vec<String> {
        match self.follows.read() {
            Ok(guard) => guard.iter().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// A follow predicate: `true` iff `pubkey` is in the active account's
    /// follow set (including the active account itself).
    ///
    /// The returned closure captures a **clone of the internal
    /// `Arc<RwLock<…>>`**, so a predicate handed out before a kind:3 update
    /// (or an account switch) reflects that update *live*. This is the
    /// load-bearing property of the closure-only design (§3-D): the engine
    /// holds the predicate, the producer mutates the shared set, and the
    /// engine's view stays current with zero re-wiring.
    ///
    /// A poisoned lock degrades the predicate to `false` for every pubkey
    /// (D6) — fail-closed: an event whose author cannot be confirmed as a
    /// follow is dropped, never surfaced.
    #[must_use]
    pub fn predicate(&self) -> Arc<dyn Fn(&str) -> bool + Send + Sync> {
        let follows = Arc::clone(&self.follows);
        Arc::new(move |pubkey: &str| match follows.read() {
            Ok(guard) => guard.contains(pubkey),
            Err(_) => false,
        })
    }

    /// Register a callback fired on every follow-set change — kind:3 update,
    /// account switch, and logout.
    ///
    /// Callbacks fire after the set is rebuilt, so a callback that reads
    /// [`ActiveFollowSet::follows`] sees the new state. Poisoned callback-
    /// registry lock → the callback is silently dropped (D6); registration is
    /// best-effort and never panics.
    pub fn on_change(&self, callback: ChangeCallback) {
        if let Ok(mut callbacks) = self.on_change.lock() {
            callbacks.push(callback);
        }
    }

    /// Notify the producer that the active account changed (switch or logout).
    ///
    /// Re-reads the [`ActiveAccountSlot`] and rebuilds the set for the new
    /// active account:
    /// * **Switch** — the new active account's own pubkey seeds the set; the
    ///   prior account's follows are cleared. The new account's kind:3 (re-
    ///   fetched by the kernel on switch) repopulates the rest as it arrives.
    /// * **Logout** (slot is `None`) — the set is cleared entirely; the
    ///   predicate returns `false` for everyone.
    ///
    /// Fires `on_change` unconditionally (the active account changed, which is
    /// itself an observable transition even if the resulting set is empty).
    ///
    /// This is the explicit account-change seam: [`ActiveAccountSlot`] carries
    /// no push notification, so the composition root (rung 6) calls this from
    /// the identity-change path.
    pub fn notify_account_changed(&self) {
        self.rebuild_for_active_account();
        self.fire_on_change();
    }

    /// Seed the set with the active account's own pubkey (self-inclusion),
    /// without firing callbacks. Used at construction.
    fn seed_self(&self) {
        let active = active_pubkey(&self.active_pubkey);
        if let Ok(mut guard) = self.follows.write() {
            guard.clear();
            if let Some(me) = active {
                guard.insert(me);
            }
        }
    }

    /// Rebuild the set for the *current* active account: clear, then re-seed
    /// self-inclusion. Follows from the new account's kind:3 arrive later via
    /// the observer and are layered on top. Does not fire callbacks (the
    /// caller decides when to fire).
    fn rebuild_for_active_account(&self) {
        self.seed_self();
    }

    /// Fire every registered `on_change` callback. Poisoned registry lock →
    /// silent no-op (D6).
    fn fire_on_change(&self) {
        let callbacks = match self.on_change.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        for cb in callbacks.iter() {
            cb();
        }
    }
}

/// Read the active account's hex pubkey from the slot, or `None` when no
/// account is signed in or the lock is poisoned (D6).
fn active_pubkey(slot: &ActiveAccountSlot) -> Option<String> {
    match slot.lock() {
        Ok(guard) => guard.as_ref().cloned(),
        Err(_) => None,
    }
}

impl KernelEventObserver for ActiveFollowSet {
    /// Called by the kernel once per accepted kind:3 event.
    ///
    /// Gate by `kind == 3` **and** author == active pubkey, then rebuild the
    /// set from the event's `p`-tagged pubkeys plus the active account's own
    /// pubkey (self-inclusion). Fires `on_change` on a successful rebuild.
    ///
    /// # Why the author gate
    ///
    /// The set only ever describes the *active* account's follows. kind:3
    /// events authored by anyone else (e.g. profiles surfaced in the follow
    /// feed) must not mutate the set — the same shadow-storage concern the
    /// sibling `FollowListProjection` guards against. On account switch the
    /// kernel re-fetches the new active account's kind:3, so the new follow
    /// list repopulates on its own (and `notify_account_changed` clears the
    /// stale entries first).
    ///
    /// Poisoned mutex / no active account → silent no-op (D6).
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_CONTACT_LIST {
            return;
        }

        // Author gate: only the active account's kind:3 mutates the set.
        let active = match active_pubkey(&self.active_pubkey) {
            Some(pk) => pk,
            None => return,
        };
        if active != event.author {
            return;
        }

        let mut rebuilt: BTreeSet<String> = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().is_some_and(|t| t == "p") {
                    tag.get(1).cloned()
                } else {
                    None
                }
            })
            .collect();
        // Self-inclusion: the active account's own pubkey is always a member.
        rebuilt.insert(active);

        {
            let Ok(mut guard) = self.follows.write() else {
                return;
            };
            *guard = rebuilt;
        }
        self.fire_on_change();
    }
}

#[cfg(test)]
#[path = "active_follow_set/tests.rs"]
mod tests;
