//! `register_op_feed_defaults` — the V-80 rung 6 (Stage 5) composition root
//! that wires the OP-centric home feed together.
//!
//! This is the one place in the system that names `NmpApp` (`nmp-ffi`) and the
//! NIP-10 OP-feed instance (`nmp-nip01`) in the same breath. Every lower layer
//! deliberately avoids that edge: `nmp-feed` is generic, `nmp-nip01`'s
//! `register_op_feed` returns an `Arc<OpFeedEngine>` for *someone else* to
//! register, and `nmp-nip02`'s `ActiveFollowSet` takes an
//! [`ActiveAccountSlot`], not `&NmpApp`. The composition root closes the loop.
//!
//! # What this function wires
//!
//! 1. Constructs [`nmp_nip02::ActiveFollowSet`] over the kernel's
//!    [`ActiveAccountSlot`] (the producer of the follow predicate).
//! 2. Builds the four closures `register_op_feed` needs:
//!    * **follow predicate** — `active_follow_set.predicate()` (live view of
//!      the active account's follow set);
//!    * **event lookup** — a no-op `|_| None` this rung (see
//!      [the event-lookup note](#why-event_lookup-is-a-no-op-this-rung));
//!    * **claim sink** — `nmp_nip01::op_feed::build_actor_claim_sink` over a
//!      dispatcher built from `app.actor_sender()` (the public command-send
//!      seam; `NmpApp::send_cmd` is crate-private);
//!    * **card builder** — supplied inside `register_op_feed` itself
//!      (`TimelineEventCard::from_event_for_op_feed`).
//! 3. Registers the returned `Arc<OpFeedEngine>` as a
//!    [`KernelEventObserver`](nmp_core::KernelEventObserver) (ingest) **and** as
//!    a [`FeedController`](nmp_feed::FeedController) under
//!    `"nmp.feed.home"` (output).
//! 4. Registers the `ActiveFollowSet` as its own `KernelEventObserver` (so
//!    kind:3 ingest keeps the follow set current — exactly the pattern the
//!    sibling `FollowListProjection` already uses).
//! 5. Registers an `on_change` callback that resets the engine **only on an
//!    account switch** (see [the account-switch note](#account-switch-vs-kind3-update)).
//!
//! # CRITICAL DECISION — no per-follow interest expansion here
//!
//! The design doc (`docs/perf/op-centric-feed-architecture.md` §3-D / §5
//! Stage 5) and [ADR-0036](../../docs/decisions/0036-composition-root-followset-expansion.md)
//! sketch an `expand_follow_timeline_interests` that registers one
//! `LogicalInterest` per follow at the composition root, "mirroring the
//! kernel's existing `sync_follow_feed_interests` semantics."
//!
//! **That mirror is a bug, so this function deliberately does NOT do it.** The
//! kernel still owns `sync_follow_feed_interests`
//! (`crates/nmp-core/src/kernel/ingest/contacts.rs:119`): on the active
//! account's kind:3 (`ingest_contacts`) and on every identity change
//! (`register_follow_feed_for_active_account` /
//! `reconcile_follow_feed_after_identity_change`) it registers one per-follow
//! `LogicalInterest` (host-declared kind:1/6) AND rebuilds `timeline_authors`.
//! Those subscriptions are what bring the follow-feed kind:1/6 events onto the
//! wire; the OP-feed engine then observes them via the kernel's
//! `KernelEventObserver` fan-out. Registering the same interests **again** at
//! the composition root would be duplicate REQ subscriptions — a wire-level
//! bug and a no-duplication-rule violation.
//!
//! The design doc predates the kernel keeping `sync_follow_feed_interests`
//! (the v3→v4 override deleted the planner-side `SocialTimeline` seam but the
//! kernel-side per-follow expansion was never removed — it is still the live
//! producer of the follow-feed subscription). The composition root therefore
//! only needs to wire the **engine** (predicate + event_lookup + claim sink +
//! card builder) and the `ActiveFollowSet` `on_change`; no interest expansion.
//!
//! # Why `event_lookup` is a no-op this rung
//!
//! The engine's `event_lookup: Arc<dyn Fn(&EventId) -> Option<KernelEvent>>` is
//! consulted only by the repost L-2/L-5 rebuild paths to read a parent/target
//! event the engine has not yet observed. There is **no synchronous
//! event-by-id read API on `NmpApp`** — the kernel's `EventStore`
//! (`get_by_id`) lives on the actor thread and is never published back to
//! `NmpApp`. A `|_| None` lookup is correctness-preserving: the engine's L-2
//! fallback holds the attribution against the wrapper id and re-keys it when
//! the wrapper later arrives via the observer fan-out (§3-L step 2); L-5 simply
//! shows the placeholder card until the target arrives. The optimization (skip
//! the round-trip when the event is already cached) is deferred — see the
//! BACKLOG `V-83` TODO.
//!
//! # Why the constructor takes `ActiveAccountSlot`, not an `NmpApp` accessor
//!
//! `ActiveFollowSet::new` needs the kernel's [`ActiveAccountSlot`]. `NmpApp`
//! does not expose one: the kernel constructs its `active_account_handle`
//! internally (`crates/nmp-core/src/kernel/mod.rs:1406`) and never threads a
//! clone back to `NmpApp` (unlike `relay_edit_rows`, which `NmpApp` owns and
//! injects). Adding an `NmpApp::active_account_handle()` accessor would require
//! threading the slot through `run_actor_with_observers` and binding it onto
//! the kernel — an `nmp-core`/actor change beyond rung 6's scope. So the slot
//! is an explicit parameter; rung 7 (the Chirp cut-over) obtains it however it
//! reaches the kernel, and the accessor is filed as BACKLOG `V-82`.
//!
//! # Account switch vs kind:3 update
//!
//! `ActiveFollowSet::on_change` fires on **both** a kind:3 update and an
//! account switch (`notify_account_changed`). They need different engine
//! responses:
//!
//! * **kind:3 update** — the predicate is *live* (it captures a clone of the
//!   `ActiveFollowSet`'s internal `Arc<RwLock<…>>`), so the engine needs
//!   nothing: future fan-out is already gated by the new follow set, and stale
//!   roots D5-evict naturally.
//! * **account switch** — the engine holds roots/attributions built from the
//!   *prior* account's events; it MUST be reset
//!   ([`OpFeedEngine::reset_for_identity_change`]).
//!
//! `on_change` cannot tell the two apart, so the callback **self-detects**
//! against the slot: it remembers the last-seen active pubkey and resets the
//! engine only when the pubkey actually changed. `last_seen` is initialised
//! from the slot at registration, so the first post-startup kind:3 fire is not
//! a false positive.
//!
//! ## The account-change race (rung-4 flagged this)
//!
//! On a switch A → B the host (rung 7) writes B into the slot and then calls
//! `notify_account_changed()`. `ActiveFollowSet` clears the set and re-seeds
//! self-inclusion of B (its follows are still empty — B's kind:3 has not landed
//! yet) and fires `on_change`; this callback sees `B != A`, resets the engine,
//! and records B. When B's kind:3 later ingests, `ActiveFollowSet`'s own
//! observer repopulates the set and fires `on_change` again; the callback sees
//! `B == B` and no-ops, while the predicate is now live for B's follows. The
//! clear-then-repopulate ordering means a `notify_account_changed()` issued
//! before B's kind:3 lands never rebuilds against a stale follow set — it
//! rebuilds against the empty (self-only) set and lets the kind:3 ingest fill
//! it in. **Driving `notify_account_changed()` from the real identity-change
//! path is rung 7's responsibility** (there is no account-switch push seam at
//! the composition root today); this rung wires the safe-clear behaviour and
//! tests the switch→clear→kind:3→repopulate sequence directly.
//!
//! [`ActiveAccountSlot`]: nmp_core::slots::ActiveAccountSlot

use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::{ActorCommand, KernelEventObserver};
use nmp_feed::FeedController;
use nmp_ffi::NmpApp;
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::op_feed::{build_actor_claim_sink, register_op_feed, ActorCommandDispatch};
use nmp_nip01::OpFeedEngine;
use nmp_nip02::ActiveFollowSet;

/// What [`register_op_feed_defaults`] hands back to the composition caller.
///
/// Rung 6 originally returned only the `Arc<OpFeedEngine>`. Rung 7 (the Chirp
/// cut-over) also needs the `Arc<ActiveFollowSet>` so the host can drive
/// [`ActiveFollowSet::notify_account_changed`] from the real identity-change
/// path (logout / switch-before-kind:3 — the cases the kind:3-driven observer
/// does not cover). Returning both is the minimal rung-6 amendment that closes
/// the loop the module docs already telegraph ("Driving
/// `notify_account_changed()` from the real identity-change path is rung 7's
/// responsibility").
pub struct OpFeedDefaults {
    /// The registered OP-feed engine — already wired as a `KernelEventObserver`
    /// (ingest) and a `FeedController` under `"nmp.feed.home"` (output).
    pub engine: Arc<OpFeedEngine>,
    /// The follow-set producer — already wired as a `KernelEventObserver` so
    /// the active account's kind:3 keeps it current. Held by the caller so the
    /// identity-change path can call [`ActiveFollowSet::notify_account_changed`]
    /// on logout / pre-kind:3 switch.
    pub follow_set: Arc<ActiveFollowSet>,
}

/// Wire the OP-centric home feed into `app`.
///
/// Constructs the [`nmp_nip02::ActiveFollowSet`] over `active_account_slot`,
/// builds the engine via [`nmp_nip01::op_feed::register_op_feed`], and
/// registers the engine as both a [`KernelEventObserver`] (ingest) and a
/// [`FeedController`] under `"nmp.feed.home"` (output). Also registers the
/// `ActiveFollowSet` as its own `KernelEventObserver` and an `on_change`
/// callback that resets the engine on an account switch.
///
/// Returns an [`OpFeedDefaults`] carrying the `Arc<OpFeedEngine>` (so callers
/// and tests can drive the engine directly or interrogate it) and the
/// `Arc<ActiveFollowSet>` (so rung 7's host can drive
/// [`ActiveFollowSet::notify_account_changed`] on identity change). Both are
/// already registered with `app`.
///
/// **This function is NOT called by [`crate::register_defaults`] and is not
/// wired into any production app in this rung.** Rung 7 makes Chirp call it
/// (and removes the `ModularTimelineProjection` registration). Until then the
/// feed key `"nmp.feed.home"` stays owned by whatever the host registers; if a
/// host calls *both* this and `ModularTimelineProjection::register`, the
/// feed-registry is last-writer-wins (the swap is a single atomic edit in rung
/// 7, never a dual registration).
///
/// # CRITICAL DECISION
///
/// This function registers **no per-follow `LogicalInterest`s** — the kernel's
/// `sync_follow_feed_interests` already owns the follow-feed subscription.
/// Re-registering would duplicate REQ subscriptions. See the module docs.
///
/// # Ordering
///
/// Like [`crate::register_defaults`], call before `nmp_app_start`: the engine
/// and the follow-set observer must be visible to the kernel when the first
/// event arrives.
pub fn register_op_feed_defaults(
    app: &NmpApp,
    viewer: Pubkey,
    active_account_slot: ActiveAccountSlot,
) -> OpFeedDefaults {
    // ── 1. Follow-set producer ───────────────────────────────────────────
    //
    // Constructed over the kernel's active-account slot (NOT `&NmpApp` — see
    // module docs). Self-seeds the active account's own pubkey immediately.
    let follow_set = nmp_nip02::ActiveFollowSet::new(active_account_slot.clone());

    // Register the follow-set as its own `KernelEventObserver` so the active
    // account's kind:3 ingest keeps the set current. Mirrors the sibling
    // `FollowListProjection` registration in Chirp. A zero id means the
    // observer slot was poisoned — a soft-fail (the predicate degrades to the
    // self-seeded set), so we drop the id rather than abort the whole wiring.
    let follow_set_observer: Arc<dyn KernelEventObserver> = follow_set.clone();
    let _follow_set_observer_id = app.register_event_observer(follow_set_observer);

    // ── 2. Claim sink dispatcher ─────────────────────────────────────────
    //
    // `NmpApp::send_cmd` is crate-private; the public command-send seam is
    // `actor_sender()` -> `Sender<ActorCommand>`. Dropped sends (closed
    // channel after teardown) are best-effort no-ops (D6: a hydration request
    // is best-effort).
    let sender = app.actor_sender();
    let dispatch: ActorCommandDispatch = Arc::new(move |cmd: ActorCommand| {
        let _ = sender.send(cmd);
    });
    let claim_sink = build_actor_claim_sink(dispatch);

    // ── 3. Event lookup (no-op this rung — see module docs / BACKLOG V-83) ─
    //
    // `Fn(&EventId) -> Option<KernelEvent>`. No synchronous event-by-id read
    // API exists on `NmpApp`; the engine's L-2 fallback (re-key on later
    // observer arrival) keeps the no-op correctness-preserving.
    let event_lookup: nmp_feed::EventLookup = Arc::new(|_id| None);

    // ── 4. Construct the engine ──────────────────────────────────────────
    let engine = register_op_feed(viewer, follow_set.predicate(), event_lookup, claim_sink);

    // ── 5. Register the engine (ingest + output) ─────────────────────────
    let engine_observer: Arc<dyn KernelEventObserver> = engine.clone();
    let _engine_observer_id = app.register_event_observer(engine_observer);
    let engine_feed: Arc<dyn FeedController> = engine.clone();
    app.register_feed(nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY, engine_feed);

    // ── 6. Account-switch reset (NOT on kind:3 updates) ──────────────────
    //
    // `on_change` fires on both a kind:3 update and an account switch. The
    // predicate is live, so a kind:3 update needs no engine action; only an
    // account switch (the active pubkey actually changed) requires a reset.
    // The callback self-detects against the slot, seeded with the
    // registration-time active pubkey so the first kind:3 fire is not a false
    // positive. See the module docs for the full switch race analysis.
    let last_seen = Arc::new(Mutex::new(read_active(&active_account_slot)));
    let engine_for_cb = engine.clone();
    let slot_for_cb = active_account_slot;
    follow_set.on_change(Box::new(move || {
        let current = read_active(&slot_for_cb);
        let Ok(mut last) = last_seen.lock() else {
            return;
        };
        if *last != current {
            *last = current;
            engine_for_cb.reset_for_identity_change();
        }
    }));

    OpFeedDefaults { engine, follow_set }
}

/// Read the active account's hex pubkey from the slot, or `None` when no
/// account is signed in or the lock is poisoned (D6).
fn read_active(slot: &ActiveAccountSlot) -> Option<String> {
    match slot.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    }
}
