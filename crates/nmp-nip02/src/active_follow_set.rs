//! `ActiveFollowSet` — observable snapshot of the active account's follow set.
//!
//! # Overview
//!
//! The active-follow feed (V-59) needs to know, for any pubkey, whether the
//! active account follows that pubkey. The generic `RootIndexedFeed` engine in
//! `nmp-feed` (rung 3) consumes that knowledge as a closure predicate
//! (`Arc<dyn Fn(&str) -> bool + Send + Sync>`) — **not** a trait. v4 of the
//! design (see `docs/perf/op-centric-feed-architecture.md` §3-D) deleted the
//! `FollowSetLookup` trait and the `LogicalInterest::SocialTimeline` planner
//! variant in favour of a closure produced here and wired at the composition
//! root (`explicit composition`, rung 6). The rationale is recorded in
//! [ADR-0036](../../docs/decisions/0036-composition-root-followset-expansion.md).
//!
//! `ActiveFollowSet` is the **producer** of that closure. Internally it is now
//! the first consumer of `nmp_core::reactive_source_graph`: the graph takes the
//! active account and that account's contact-list follows as source inputs,
//! derives the self-included active follow set, and emits one perspective-change
//! effect when downstream consumers should reconcile acquisition, observed
//! projections, and feed windows. A small `Arc<RwLock<BTreeSet<String>>>`
//! remains as the hot predicate read cache. The graph is the only writer that
//! decides when that cache is replaced.
//!
//! * [`ActiveFollowSet::follows`] — a sorted `Vec<String>` snapshot read.
//! * [`ActiveFollowSet::predicate`] — a closure that captures a clone of the
//!   internal `Arc<RwLock<…>>`, so a predicate handed out *before* a kind:3
//!   update reflects the update *live* (the closure-only design's load-bearing
//!   property — verified by the `predicate_reflects_live_updates` test).
//! * [`ActiveFollowSet::on_source_effect`] — register an internal source-effect
//!   sink that receives graph-proven perspective changes (kind:3 update,
//!   account switch, logout).
//!
//! # Why no `&NmpApp` constructor
//!
//! The design doc sketches `ActiveFollowSet::new(app: &NmpApp)`. That is
//! pseudocode: `NmpApp` lives in `nmp-ffi`, which `nmp-nip02` depends on only
//! as a *dev*-dependency. A production `&NmpApp` parameter would invert the
//! dependency graph (`nmp-nip02 → nmp-ffi`). The substrate-clean realization —
//! mirroring the sibling [`crate::projection::FollowListProjection`] — is to
//! take the [`ActiveAccountSlot`] (re-exported through `nmp_core::slots`) and
//! a store-backed latest-kind:3 reader directly. The composition root registers this
//! struct as a `ObservedProjectionSink` separately, exactly as it already does
//! for `FollowListProjection`. No new crate edge in either direction (verified:
//! `cargo tree -p nmp-nip02` carries `nmp-core`, `nostr`, `serde`,
//! `serde_json` only — no `nmp-feed`, no `nmp-ffi`).
//!
//! # Self-inclusion
//!
//! Dynamic feed-source reduction includes the active account's *own* pubkey so
//! the user's own notes appear in their home stream. `ActiveFollowSet` mirrors
//! that inclusion: the active account's own pubkey is always a member of the
//! set (even before any kind:3 has arrived).
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
//! batches the new active account and its canonical event-store kind:3 follows
//! into the source graph (clearing it entirely on logout, when the slot is
//! `None`), and emits a source effect when the graph proves the active
//! perspective changed. A kind:3 ingest does not cover logout — there is no
//! logout-triggered kind:3 — so the explicit seam is required for correctness,
//! not convenience.
//!
//! # Compiled follow-feed acquisition kinds
//!
//! `fix(nmp-core): keep follow-feed kinds host-declared` (commit `2f06cc66`)
//! made the *follow-feed subscription* REQ kinds caller-supplied. Today that
//! set is compiled acquisition data derived above `nmp-core`. That change
//! touches which kinds the contact-list-authors REQ carries; it does **not**
//! touch the kind:3 ingest fan-out that `ActiveFollowSet` observes. The
//! sibling `FollowListProjection` (untouched by `2f06cc66`) is the living
//! proof: kind:3 events still fan out to `ObservedProjectionSink`s gated purely on
//! `event.kind == 3` and author == active, regardless of the compiled
//! follow-feed acquisition kind set.
//!
//! # D-doctrine
//!
//! * **D0** — `nmp-nip02` is a NIP crate, so NIP-02 nouns are fine here. No
//!   NIP token leaks into `nmp-core`. The predicate is a std closure; no
//!   `nmp-feed` type appears in this crate's surface.
//! * **D5** — the set is the full kind:3 follow set (uncapped, #1497
//!   amendment 6). The kernel fans the *raw* kind:3 event (all `p` tags) to
//!   every `ObservedProjectionSink`, so this observer derives membership itself.
//!   It does so by routing the event's tags through the one shared pure function
//!   [`nmp_core::tags::contact_follows`] — the IDENTICAL recipe
//!   `Kernel::ingest_contacts` uses (every valid-hex-`p`-tag in document
//!   order). This is the single source of truth for membership; the sibling
//!   [`crate::projection::FollowListProjection`] applies the very same function
//!   so the predicate producer and the snapshot can never disagree on which
//!   follows count.
//!
//! # Sibling design divergence
//!
//! `ActiveFollowSet` and `FollowListProjection` derive the *same* (uncapped,
//! #1497) follow membership but have different internal designs:
//!
//! * `ActiveFollowSet` owns a small source graph that derives active follows
//!   from `(active account, active account contact-list follows)`. The
//!   `Arc<RwLock<BTreeSet<String>>>` is a hot predicate read cache written only
//!   from graph effects.
//! * `FollowListProjection` is a **thin read-model** over the kernel event
//!   store — it holds NO secondary `HashMap` or observer state. Its
//!   `snapshot()` reads the active account's latest kind:3 from the store, so
//!   account-switch consistency is automatic and kind:3 events remain the single
//!   source of truth. Demand interest (kind:3 acquisition) is driven by
//!   `register_follow_state_runtime` via `ActorCommand::OpenInterest` /
//!   `CloseInterest` — no `ObservedProjectionSink` registration is needed for
//!   the projection itself.
//! * **D6** — poisoned locks and a `None` active account degrade to an empty
//!   set / a `false` predicate, never a panic.
//! * **D8** — `on_kernel_event` does bounded work (one kind check, one active
//!   slot read, one `p`-tag scan, one synchronous graph turn) on the actor
//!   thread between relay frames. No I/O, no blocking, no polling.
//! * **Raw data** — the set holds raw hex pubkeys only; display formatting is
//!   a higher-layer concern (2026-05-25 display-separation doctrine).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, RwLock};

use nmp_core::kinds::KIND_CONTACT_LIST;
use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::KernelEvent;
use nmp_core::tags::contact_follows;
use nmp_core::ObservedProjectionSink;

use crate::LatestKind3FollowSet;

mod reactive_graph;

use reactive_graph::{ActiveFollowGraph, ActiveFollowGraphEffect};

/// Source effect emitted by the active-follow graph after it has proven that
/// the active perspective changed and the predicate read cache has been
/// updated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActiveFollowSourceEffect {
    PerspectiveChanged { follows: BTreeSet<String> },
}

/// A registered graph source-effect sink. The native/browser session layer uses
/// this to reconcile acquisition, observed projections, and feed windows for
/// the active-follows first consumer.
pub type ActiveFollowSourceEffectSink = Box<dyn Fn(&ActiveFollowSourceEffect) + Send + Sync>;

/// Observable snapshot of the active account's follow set, as raw hex pubkeys.
///
/// Construct with [`ActiveFollowSet::new`] passing the kernel's
/// [`ActiveAccountSlot`] (clone of `Kernel::active_account_handle()`) and the
/// canonical latest-kind:3 reader. The composition root registers the returned
/// `Arc<Self>` as a [`ObservedProjectionSink`] so kind:3 events are ingested,
/// and calls [`ActiveFollowSet::notify_account_changed`] on identity change.
///
/// All state is `Arc`-internal so the struct is shared as `Arc<Self>` between
/// the observer registry, the composition root, source-effect sinks, and any
/// handed-out predicate.
pub struct ActiveFollowSet {
    /// The active account's hex pubkey slot, written by the kernel actor on
    /// account switch / logout. `None` means no signed-in account → empty set,
    /// `false` predicate.
    active_pubkey: ActiveAccountSlot,
    /// Canonical follow-set source derived from the event store's latest kind:3
    /// for an author.
    latest_kind3: LatestKind3FollowSet,
    /// Hot predicate read cache: raw hex pubkeys plus the active account's own
    /// pubkey (self-inclusion, mirroring `timeline_authors`). Captured (as an
    /// `Arc` clone) by every predicate handed out, so the predicate observes
    /// graph updates live without taking the graph mutex on every feed event.
    follows: Arc<RwLock<BTreeSet<String>>>,
    /// Internal reactive dependency graph for active account + contact-list
    /// source changes. The public predicate still reads `follows`; the graph is
    /// the only writer that decides when that read cache changes.
    graph: Mutex<ActiveFollowGraph>,
    /// Registered graph source-effect sinks, fired on every perspective
    /// transition after the predicate read cache is updated.
    source_effect_sinks: Mutex<Vec<ActiveFollowSourceEffectSink>>,
}

impl ActiveFollowSet {
    /// Construct an `ActiveFollowSet` over the kernel's active-account slot and
    /// canonical latest-kind:3 reader.
    ///
    /// Returns `Arc<Self>` because the same value is shared three ways: as a
    /// [`ObservedProjectionSink`] in the kernel's observer registry, as the
    /// source-effect owner the session compiler drives, and as the source of
    /// the captured `Arc<RwLock<…>>` inside every handed-out predicate.
    ///
    /// The set is seeded immediately from the slot's current value and cached
    /// contacts, so a predicate handed out before any kind:3 arrives still
    /// returns `true` for the active account's own pubkey and for any
    /// sign-in-prepopulated follows.
    #[must_use]
    pub fn new(active_pubkey: ActiveAccountSlot, latest_kind3: LatestKind3FollowSet) -> Arc<Self> {
        let initial_active = active_pubkey_from_slot(&active_pubkey);
        let initial_contacts = contact_follows_for(&latest_kind3, initial_active.as_deref());
        let graph = ActiveFollowGraph::new(initial_active, initial_contacts);
        let follows = graph.current_follows();
        let this = Arc::new(Self {
            active_pubkey,
            latest_kind3,
            follows: Arc::new(RwLock::new(follows)),
            graph: Mutex::new(graph),
            source_effect_sinks: Mutex::new(Vec::new()),
        });
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

    /// Register an internal source-effect sink fired on every graph-proven
    /// perspective change — kind:3 update, account switch, and logout.
    ///
    /// Sinks fire after the graph has updated the predicate read cache, so a
    /// sink that reads [`ActiveFollowSet::follows`] sees the new state. Poisoned
    /// sink-registry lock → the sink is silently dropped (D6); registration is
    /// best-effort and never panics.
    pub fn on_source_effect(&self, sink: ActiveFollowSourceEffectSink) {
        if let Ok(mut sinks) = self.source_effect_sinks.lock() {
            sinks.push(sink);
        }
    }

    /// Notify the producer that the active account changed (switch or logout).
    ///
    /// Re-reads the [`ActiveAccountSlot`] and applies one graph turn for the
    /// new active account:
    /// * **Switch** — the prior account's follows are cleared, then the new
    ///   account's cached contacts and own pubkey are loaded immediately. Later
    ///   kind:3 ingest keeps the set current.
    /// * **Logout** (slot is `None`) — the set is cleared entirely; the
    ///   predicate returns `false` for everyone.
    ///
    /// Emits a source effect only when the active perspective changes. A
    /// duplicate identity notification for the same account, or an unchanged
    /// cache hydrate, must not force downstream feed resets.
    ///
    /// This is the explicit account-change seam: [`ActiveAccountSlot`] carries
    /// no push notification, so the composition root (rung 6) calls this from
    /// the identity-change path.
    pub fn notify_account_changed(&self) {
        let active = active_pubkey_from_slot(&self.active_pubkey);
        let contacts = contact_follows_for(&self.latest_kind3, active.as_deref());
        let effects = self.apply_graph_active_source(active, contacts);
        self.apply_graph_effects(effects);
    }

    fn apply_graph_active_source(
        &self,
        active: Option<String>,
        contacts: BTreeSet<String>,
    ) -> Vec<ActiveFollowGraphEffect> {
        match self.graph.lock() {
            Ok(mut graph) => graph.apply_active_source(active, contacts),
            Err(_) => Vec::new(),
        }
    }

    fn apply_graph_effects(&self, effects: Vec<ActiveFollowGraphEffect>) {
        let mut source_effects = Vec::new();
        for effect in effects {
            match effect {
                ActiveFollowGraphEffect::PerspectiveChanged { follows } => {
                    if self.replace_follows_snapshot(follows.clone()) {
                        source_effects
                            .push(ActiveFollowSourceEffect::PerspectiveChanged { follows });
                    }
                }
            }
        }
        if !source_effects.is_empty() {
            self.fire_source_effects(&source_effects);
        }
    }

    /// Replace the predicate read cache after the graph proves a perspective
    /// change. Returns `false` for poisoned locks so callers fail closed without
    /// firing source effects against stale data.
    fn replace_follows_snapshot(&self, rebuilt: BTreeSet<String>) -> bool {
        match self.follows.write() {
            Ok(mut guard) => {
                *guard = rebuilt;
                true
            }
            Err(_) => false,
        }
    }

    /// Fire every registered source-effect sink. Poisoned registry lock →
    /// silent no-op (D6).
    fn fire_source_effects(&self, effects: &[ActiveFollowSourceEffect]) {
        let sinks = match self.source_effect_sinks.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        for effect in effects {
            for sink in sinks.iter() {
                sink(effect);
            }
        }
    }
}

/// Read the active account's hex pubkey from the slot, or `None` when no
/// account is signed in or the lock is poisoned (D6).
fn active_pubkey_from_slot(slot: &ActiveAccountSlot) -> Option<String> {
    match slot.lock() {
        Ok(guard) => guard.as_ref().cloned(),
        Err(_) => None,
    }
}

impl ObservedProjectionSink for ActiveFollowSet {
    /// Called by the kernel once per accepted kind:3 event.
    ///
    /// Gate by `kind == 3` **and** author == active pubkey, then apply one
    /// graph turn for `(active pubkey, event p-tagged follows)`. Emits a source
    /// effect when the graph proves the active perspective changed.
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
        let active = match active_pubkey_from_slot(&self.active_pubkey) {
            Some(pk) => pk,
            None => return,
        };
        if active != event.author {
            return;
        }

        // Derive the follow set through the one shared pure function
        // (`contact_follows`): every valid-hex `p`-tag in document order — the
        // IDENTICAL set the router subscribes to in `Kernel::ingest_contacts`.
        // The follow set is uncapped (#1497 amendment 6): the follow-feed is one
        // multi-author interest covering every follow, so the predicate and the
        // wire subscription cover the same authors. The shared function dedups
        // nothing and preserves order; the `BTreeSet` here de-duplicates and
        // sorts for membership lookup.
        let contacts: BTreeSet<String> = contact_follows(&event.tags).into_iter().collect();
        let effects = self.apply_graph_active_source(Some(active), contacts);
        self.apply_graph_effects(effects);
    }
}

fn contact_follows_for(
    latest_kind3: &LatestKind3FollowSet,
    active: Option<&str>,
) -> BTreeSet<String> {
    active
        .and_then(|author| latest_kind3.follows(author))
        .unwrap_or_default()
        .into_iter()
        .collect()
}

#[cfg(test)]
#[path = "active_follow_set/tests.rs"]
mod tests;
