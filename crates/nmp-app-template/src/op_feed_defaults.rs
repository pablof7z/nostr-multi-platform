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
//! # `event_lookup` reads the kernel event store (V-83)
//!
//! The engine's `event_lookup: Arc<dyn Fn(&EventId) -> Option<KernelEvent>>` is
//! consulted by the repost L-2/L-5 rebuild paths to read a parent/target/wrapper
//! event the engine has not yet observed but the kernel has already cached:
//!
//! * **L-5** (`OpFeedEngine::ingest_root`): a kind:6 repost wrapper keyed the
//!   target id first (placeholder card); when the target arrives, the engine
//!   re-fetches the **wrapper** via `event_lookup` to rebuild the card from the
//!   `(wrapper, target)` pair so the "reposted by" provenance survives. Without
//!   a real lookup the card rebuilds from `(target, None)` — provenance lost.
//! * **L-2** (`OpFeedEngine::ingest_reply`): a reply points at a repost wrapper;
//!   the engine looks the wrapper up to discover it `supersedes` a different
//!   target and re-keys the attribution onto that target instead of the wrapper.
//!
//! V-83 added [`NmpApp::event_by_id`](nmp_ffi::NmpApp::event_by_id) over the
//! kernel's published `EventStore` handle (the actor publishes
//! `Kernel::event_store_handle()` into a shared slot right after kernel
//! construction and re-publishes on `Reset` — see `nmp-ffi`). The closure here
//! captures [`NmpApp::event_store_handle`](nmp_ffi::NmpApp::event_store_handle)
//! (the slot `Arc`, NOT `&app` — the closure outlives the borrow) and reads
//! through it on every call, so a `Reset` is observed without re-capturing.
//! `EventStore::get_by_id` is a `&self` read; the actor reducer is the sole
//! writer (D4) and the store insert is ordered before the observer fan-out, so
//! a read from a `KernelEventObserver` callback (actor thread) sees the
//! just-ingested event without re-entrancy. Before `nmp_app_start` the slot is
//! empty → `None`, which is exactly the prior no-op behaviour (still
//! correctness-preserving: the L-2 fallback re-keys on a later observer arrival
//! and L-5 shows the placeholder until the target lands).
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
/// [`FeedController`] under `"nmp.feed.home"` (output). Also registers a typed
/// `NOFS` sidecar projection under the same key (ADR-0038 T1) ALONGSIDE the
/// generic `Value` `FeedController` — a host with a `NOFS` decoder prefers the
/// typed payload, others fall back to the generic `Value` subtree. Finally
/// registers the `ActiveFollowSet` as its own `KernelEventObserver` and an
/// `on_change` callback that resets the engine on an account switch.
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

    // ── 3. Event lookup (V-83 — real synchronous kernel event read) ──────
    //
    // `Fn(&EventId) -> Option<KernelEvent>`. The engine's repost L-2/L-5
    // backward-hydration paths consult this to read a parent/target/wrapper
    // event the engine has not yet observed but the kernel has already cached.
    // V-83 added `NmpApp::event_by_id` over the kernel's published `EventStore`
    // handle (`event_store_handle()` returns the shared `Arc` slot the actor
    // publishes into — see `nmp-ffi`). The closure captures the slot handle (NOT
    // `&app`, which it would outlive) and reads through it on every call, so a
    // `Reset` (which re-publishes a fresh store into the same slot) is observed
    // without re-capturing. Pre-`nmp_app_start` the slot is empty → `None`,
    // which is exactly the prior no-op behaviour, so wiring is safe before the
    // kernel exists. Mirrors V-82's slot-capture in the `on_change` callback
    // below.
    let event_store = app.event_store_handle();
    let event_lookup: nmp_feed::EventLookup = Arc::new(move |id: &nmp_core::substrate::EventId| {
        nmp_core::slots::event_by_id_from_store(&event_store, id)
    });

    // ── 4. Construct the engine ──────────────────────────────────────────
    let engine = register_op_feed(viewer, follow_set.predicate(), event_lookup, claim_sink);

    // ── 5. Register the engine (ingest + output) ─────────────────────────
    let engine_observer: Arc<dyn KernelEventObserver> = engine.clone();
    let _engine_observer_id = app.register_event_observer(engine_observer);
    let engine_feed: Arc<dyn FeedController> = engine.clone();
    app.register_feed(nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY, engine_feed);

    // ── 5b. Register the typed NOFS sidecar (ADR-0038 Commitment 5) ───────
    //
    // Emit the typed FlatBuffers `OpFeedSnapshot` (`schema_id
    // "nmp.nip01.opfeed"`, `file_identifier "NOFS"`) ALONGSIDE the generic
    // `Value` `FeedController` registration above. A host with a `NOFS` decoder
    // prefers this typed payload; an un-updated host sees an unrecognized
    // descriptor and falls back to the generic `Value` subtree (the permanent
    // fallback from PR #747). Additive — un-updated hosts are unaffected.
    //
    // Known waste, deferred (ADR-0038 Commitment 5): this closure snapshots the
    // engine again on the same tick the `FeedController` path snapshots it (two
    // window materializations per 4 Hz tick). Not load-bearing for correctness;
    // a shared per-tick snapshot cache is a tracked follow-up.
    let engine_for_typed = Arc::clone(&engine);
    app.register_typed_snapshot_projection(
        nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY,
        move || {
            // ADR-0038 open-Q1 default: the typed sidecar mirrors the default
            // window (matches the diagnostics-handle path); viewport-aware
            // typed emit is a follow-up tied to the staged-removal close.
            let snapshot = engine_for_typed.snapshot(&nmp_feed::FeedRequest::default());
            Some(nmp_core::TypedProjectionData {
                key: nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY.to_string(),
                schema_id: nmp_nip01::op_feed::OP_FEED_SCHEMA_ID.to_string(),
                schema_version: nmp_nip01::op_feed::OP_FEED_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(
                    nmp_nip01::op_feed::OP_FEED_FILE_IDENTIFIER,
                )
                .into_owned(),
                payload: nmp_nip01::op_feed::encode_op_feed_snapshot(&snapshot),
            })
        },
    );

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
