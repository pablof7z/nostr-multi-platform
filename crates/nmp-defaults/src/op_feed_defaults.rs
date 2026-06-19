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
//!    * **event lookup** — a synchronous read through the kernel event-store
//!      handle exposed by `NmpApp`;
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
//! 6. Registers the follow-set notifier on `NmpApp`'s identity-change observer
//!    seam so sign-in, switch, logout, and reset are pushed after the actor has
//!    written the active-account slot.
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
//! card builder), the `ActiveFollowSet` `on_change`, and the app-level
//! identity-change callback; no interest expansion.
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
//! # Active-account source of truth
//!
//! `ActiveFollowSet::new` needs the kernel's [`ActiveAccountSlot`].
//! `register_op_feed_defaults` reads it directly from
//! [`NmpApp::active_account_handle`](nmp_ffi::NmpApp::active_account_handle)
//! so the follow predicate and the identity-change observer share the same
//! app-owned `Arc` the actor writes in `Kernel::set_accounts`.
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
//! On a switch A → B the actor updates the active-account slot, emits a state
//! frame, and `NmpApp`'s update listener fires its identity observers before
//! forwarding that frame to native. The callback registered here calls
//! `notify_account_changed()`: `ActiveFollowSet` clears the set and re-seeds
//! self-inclusion of B (its follows are still empty — B's kind:3 has not landed
//! yet) and fires `on_change`; this callback sees `B != A`, resets the engine,
//! and records B. When B's kind:3 later ingests, `ActiveFollowSet`'s own
//! observer repopulates the set and fires `on_change` again; the callback sees
//! `B == B` and no-ops, while the predicate is now live for B's follows. The
//! clear-then-repopulate ordering means the switch-before-kind:3 window never
//! rebuilds against a stale follow set.
//!
//! [`ActiveAccountSlot`]: nmp_core::slots::ActiveAccountSlot

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use nmp_planner::InterestShape;
use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::KernelEvent;
use nmp_core::{ActorCommand, KernelEventObserver};
use nmp_feed::{ClosureInterestShape, FeedAdvance, FeedApply, FeedController, PullFeedController};
use nmp_ffi::NmpApp;
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::op_feed::{
    build_actor_claim_sink, register_op_feed, ActorCommandDispatch, FeedEmissionState, FrameIdentity,
};
use nmp_nip01::OpFeedEngine;
use nmp_nip02::ActiveFollowSet;

/// What [`register_op_feed_defaults`] hands back to the composition caller.
///
/// Returns both registered pieces so tests and diagnostic callers can inspect
/// the engine and the follow-set producer. Production identity changes are
/// driven through the `NmpApp` observer registered inside
/// [`register_op_feed_defaults`]; callers do not manually notify the follow set.
pub struct OpFeedDefaults {
    /// The registered OP-feed engine — already wired as a `KernelEventObserver`
    /// (ingest) and a `FeedController` under `"nmp.feed.home"` (output).
    pub engine: Arc<OpFeedEngine>,
    /// The follow-set producer — already wired as a `KernelEventObserver` for
    /// kind:3 updates and as an `NmpApp` identity observer for sign-in, switch,
    /// logout, and reset.
    pub follow_set: Arc<ActiveFollowSet>,
}

/// Wire the OP-centric home feed into `app`.
///
/// Constructs the [`nmp_nip02::ActiveFollowSet`] over the app's active-account slot,
/// builds the engine via [`nmp_nip01::op_feed::register_op_feed`], and
/// registers the engine as both a [`KernelEventObserver`] (ingest) and a
/// [`FeedController`] under `"nmp.feed.home"` (output). Also registers a typed
/// `NOFS` sidecar projection under the same key (ADR-0038 T1) ALONGSIDE the
/// generic `Value` `FeedController` — a host with a `NOFS` decoder prefers the
/// typed payload, others fall back to the generic `Value` subtree. Finally
/// registers the `ActiveFollowSet` as its own `KernelEventObserver` and an
/// `on_change` callback that resets the engine on an account switch.
///
/// Returns an [`OpFeedDefaults`] carrying the `Arc<OpFeedEngine>` and
/// `Arc<ActiveFollowSet>` for direct tests/diagnostics. Both are already
/// registered with `app`, including identity-change notification.
///
/// Chirp calls this during app registration to own the home feed key. A host
/// must not also register a legacy home-feed producer under `"nmp.feed.home"`.
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
    contact_feed_kinds: Vec<u32>,
) -> OpFeedDefaults {
    // ── 1. Follow-set producer ───────────────────────────────────────────
    //
    // Constructed over the kernel's active-account slot exposed by `NmpApp`.
    // Self-seeds the active account's own pubkey immediately.
    let active_account_slot = app.active_account_handle();
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
    // without re-capturing. Pre-`nmp_app_start` the slot is empty → `None`, so
    // wiring is safe before the kernel exists; the L-2/L-5 paths re-check on the
    // next event arrival.
    let event_store = app.event_store_handle();
    let event_lookup: nmp_feed::EventLookup = Arc::new(move |id: &nmp_core::substrate::EventId| {
        nmp_core::slots::event_by_id_from_store(&event_store, id)
    });

    // ── 4. Construct the engine ──────────────────────────────────────────
    let engine = register_op_feed(viewer.clone(), follow_set.predicate(), event_lookup, claim_sink);

    // ── 5. Register the engine (ingest + output) ─────────────────────────
    let engine_observer: Arc<dyn KernelEventObserver> = engine.clone();
    let _engine_observer_id = app.register_event_observer(engine_observer);

    // ── 5a. Wire the home feed to the seq-ordered pull pager (ADR-0058 §8 6B) ──
    //
    // `load_older` is no longer a `created_at` window-grow on the engine (that
    // parallel path was deleted in 6B — the engine is no longer a
    // `FeedController`). It is now a synchronous, on-demand pull drain over the
    // kernel's ingest log, via `nmp_feed::PullFeedController`:
    //
    //   * **interest** — a LIVE, fail-closed `InterestShape` recomputed on every
    //     `load_older`: the active account's follow set + the active user as
    //     `authors`, under the host-declared `contact_feed_kinds`. This is the
    //     same collapsed follow-feed shape the kernel registers for M2
    //     (`sync_follow_feed_interests`). The controller is registered
    //     UNCONDITIONALLY (below); the provider proves a LIVE active account
    //     FIRST and yields `None` on empty kinds or no signed-in account, so a
    //     `load_older` after logout/switch fails closed (no pull, never a
    //     broad-scan; D5) even while a stale follow set lingers.
    //   * **pull** — `app.feed_pull_fn()`, an in-process read over the kernel
    //     event store (NOT a new host pull accessor; ADR-0039 §6.1 preserved).
    //   * **apply** — the engine's own `KernelEventObserver` ingest path, so a
    //     drained page deduplicates and projects exactly like live push ingest.
    //   * **advance** — `grow_visible_window`, the render-viewport step that
    //     reveals the just-ingested `(created_at, id)`-sorted roots one page at a
    //     time. Completeness rides ingest seq; display order is unchanged.
    let provider: Arc<dyn nmp_feed::FeedInterestShape + Send + Sync> = {
        let follow_set = follow_set.clone();
        // B2: capture the live active-account SLOT (not the registration-time
        // viewer pubkey) so the closure reads the CURRENT signed-in account on
        // every load_older call. After logout the slot holds None ⇒ authors is
        // empty ⇒ provider returns None ⇒ load_older fails closed. After an
        // account switch the slot holds the new pubkey without re-registering.
        let account_slot = active_account_slot.clone();
        let kinds: BTreeSet<u32> = contact_feed_kinds.iter().copied().collect();
        Arc::new(ClosureInterestShape::new(move || {
            live_contact_feed_shape(&account_slot, &follow_set, &kinds)
        }))
    };
    let pull = app.feed_pull_fn();
    let apply: FeedApply = {
        let engine = Arc::clone(&engine);
        Arc::new(move |event: &KernelEvent| engine.on_kernel_event(event))
    };
    let advance: FeedAdvance = {
        let engine = Arc::clone(&engine);
        Arc::new(move || {
            engine.grow_visible_window();
        })
    };
    // B2: register UNCONDITIONALLY — PullFeedController no longer requires an
    // initial shape. The provider re-reads the live shape on every load_older;
    // None from the provider fails closed (no pull, no broad-scan). A controller
    // registered before sign-in becomes active as soon as the user signs in.
    {
        let controller: Arc<dyn FeedController> =
            PullFeedController::new(provider, pull, apply, advance);
        app.register_feed(nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY, controller);
    }

    // ── 5b. Register the typed NOFS sidecar (ADR-0038 Commitment 5) ───────
    //
    // ADR-0055 Rung 6 Option A (R6-S1): the typed sidecar now uses
    // `FeedEmissionState` to omit an unchanged feed frame when the host has
    // declared incremental-apply capability (exact byte equality, monotonic rev).
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
    //
    // R6-S1 emission state + frame-identity rebaseline (the freeze fix):
    //
    // The producer rebaselines on the EXACT signal the host's `ProjectionCache`
    // resets on: the frame `(session_id, snapshot_epoch)` tuple. The kernel
    // publishes this each tick (before any projection closure runs) into shared
    // `Arc<AtomicU64>` handles; the closure reads them lock-free and forces a
    // full baseline whenever either component changes. This covers account-switch
    // AND `ActorCommand::Reset` AND any future epoch-class bump with ONE durable
    // signal — there is no bespoke per-event epoch counter (the prior
    // `emission_epoch` bumped from `follow_set.on_change` was blind to Reset and
    // is deleted).
    let engine_for_typed = Arc::clone(&engine);
    let incremental_apply = app.incremental_apply_handle();
    let (frame_session_id, frame_snapshot_epoch) = app.frame_identity_handles();
    // `FeedEmissionState` is NOT `Send` (it holds `Vec<u8>` and is owned by the
    // closure), but the closure itself must be `Send + Sync` as required by
    // `register_typed_snapshot_projection`. We wrap the state in a `Mutex` to
    // satisfy the `Sync` bound; the lock is uncontested in production (only the
    // actor thread calls the closure under the registry's own mutex).
    let emission_state = Arc::new(Mutex::new(FeedEmissionState::new(incremental_apply)));
    app.register_typed_snapshot_projection(nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY, move || {
        // ADR-0038 — the typed sidecar MUST reflect the CURRENT window,
        // including any pages revealed by prior `load_older` calls. Using
        // `FeedRequest::default()` is lossless only on the first page; after
        // `load_older` grows `window_limit` the default request would silently
        // truncate the snapshot to the first page. `snapshot_current_window()`
        // reads the live `window_limit` counter and issues the right-sized
        // request, matching what the old JSON producer (deleted escape hatch #2)
        // did via `FeedController::snapshot_json`.
        let snapshot = engine_for_typed.snapshot_current_window();
        let payload = nmp_nip01::op_feed::encode_op_feed_snapshot(&snapshot);
        // Read this tick's frame identity lock-free (the kernel published it at
        // the top of `make_update`, before this closure runs).
        let identity = FrameIdentity {
            session_id: frame_session_id.load(Ordering::Acquire),
            snapshot_epoch: frame_snapshot_epoch.load(Ordering::Acquire),
        };

        // R6-S1: consult emission state to decide whether to emit or omit.
        let Ok(mut state) = emission_state.lock() else {
            // Poisoned mutex — degrade to always-emit (D6: safe fallback).
            return Some(nmp_core::TypedProjectionData {
                key: nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY.to_string(),
                schema_id: nmp_nip01::op_feed::OP_FEED_SCHEMA_ID.to_string(),
                schema_version: nmp_nip01::op_feed::OP_FEED_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(
                    nmp_nip01::op_feed::OP_FEED_FILE_IDENTIFIER,
                )
                .into_owned(),
                payload,
                ..Default::default()
            });
        };

        let emit_decision = state.should_emit(payload, identity);
        drop(state); // release the lock before constructing the return value

        match emit_decision {
            None => {
                // Byte-identical to last emission and capability is ON → omit.
                // The host cache retains the prior value (omit==retain invariant).
                None
            }
            Some((payload, projection_rev)) => Some(nmp_core::TypedProjectionData {
                key: nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY.to_string(),
                schema_id: nmp_nip01::op_feed::OP_FEED_SCHEMA_ID.to_string(),
                schema_version: nmp_nip01::op_feed::OP_FEED_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(nmp_nip01::op_feed::OP_FEED_FILE_IDENTIFIER)
                    .into_owned(),
                payload,
                projection_rev,
                ..Default::default()
            }),
        }
    });

    // ── 6. Account-switch reset (NOT on kind:3 updates) ──────────────────
    //
    // `on_change` fires on both a kind:3 update and an account switch. The
    // predicate is live, so a kind:3 update needs no engine action; only an
    // account switch (the active pubkey actually changed) requires a reset.
    // The callback self-detects against the slot, seeded with the
    // registration-time active pubkey so the first kind:3 fire is not a false
    // positive. See the module docs for the full switch race analysis.
    //
    // R6-S1: this callback no longer touches the typed-projection emission state.
    // An account switch bumps the kernel's `snapshot_epoch` (identity_state.rs
    // → `bump_epoch`), which the kernel publishes into the frame-identity handles
    // the typed closure reads — so the closure rebaselines automatically, in
    // lockstep with the host cache reset, with no bespoke epoch bump here.
    let last_seen = Arc::new(Mutex::new(read_active(&active_account_slot)));
    let engine_for_cb = engine.clone();
    let slot_for_cb = active_account_slot.clone();
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

    let follow_set_for_identity = follow_set.clone();
    // Identity changes are pushed from `NmpApp` after the actor has written the
    // active-account slot. This is the canonical app/FFI composition seam for
    // OP-feed account reset; hosts do not call `notify_account_changed` manually.
    app.register_identity_change_observer(move |_| {
        follow_set_for_identity.notify_account_changed();
    });

    OpFeedDefaults { engine, follow_set }
}

/// Build the LIVE contact-feed pull [`InterestShape`], or `None` to fail closed.
///
/// B1 — race-free fail-close. The active-account slot is read **first**: on
/// logout / account-switch the actor can null the slot BEFORE the async identity
/// observer clears [`ActiveFollowSet`]
/// (`crates/nmp-ffi/src/lib.rs` `update_listener`), so a synchronous
/// `load_older` can observe `slot == None` while `follow_set.follows()` is still
/// stale. Reading the slot first means no live active account ⇒ `None` ⇒ no
/// shape ⇒ no pull (never a stale-viewer pull, never a broad-scan; D5). Only
/// when there IS a live active account do we form the shape from
/// `viewer = active account pubkey` + its follows; the viewer is always a member
/// (self-inclusion), so the author set is never empty.
fn live_contact_feed_shape(
    account_slot: &ActiveAccountSlot,
    follow_set: &ActiveFollowSet,
    kinds: &BTreeSet<u32>,
) -> Option<InterestShape> {
    if kinds.is_empty() {
        return None; // host declared no contact-feed kinds ⇒ fail closed
    }
    // Prove a LIVE active account BEFORE touching the (possibly stale) follow
    // set. `None` here is the logout/switch fail-closed path.
    let viewer = read_active(account_slot)?;
    let mut authors: BTreeSet<String> = follow_set.follows().into_iter().collect();
    authors.insert(viewer);
    Some(InterestShape::timeline_for(authors, kinds.clone()))
}

/// Read the active account's hex pubkey from the slot, or `None` when no
/// account is signed in or the lock is poisoned (D6).
fn read_active(slot: &ActiveAccountSlot) -> Option<String> {
    match slot.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    }
}

#[cfg(test)]
#[path = "op_feed_defaults/tests.rs"]
mod tests;
