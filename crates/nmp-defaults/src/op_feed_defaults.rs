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
//! 2. Declares the home feed as app-owned primary kinds from the active
//!    account's reactive follows perspective. The declaration derives repost
//!    wrapper acquisition below the app boundary and queues the active-follows
//!    subscription command; it never passes concrete follow pubkeys.
//! 3. Builds the three inputs `register_op_feed` needs:
//!    * **follow predicate** — `active_follow_set.predicate()` (live view of
//!      the active account's follow set);
//!    * **event lookup** — a synchronous read through the kernel event-store
//!      handle exposed by `NmpApp`; this is a local cache read only, never
//!      acquisition;
//!    * **card builder** — supplied inside `register_op_feed` itself
//!      (`TimelineEventCard::from_event_for_op_feed`).
//! 4. Registers the returned `Arc<OpFeedEngine>` as a
//!    [`KernelEventObserver`](nmp_core::KernelEventObserver) (ingest) **and** as
//!    a [`FeedController`](nmp_feed::FeedController) under
//!    `"nmp.feed.home"` (output).
//! 5. Registers the `ActiveFollowSet` as its own `KernelEventObserver` (so
//!    kind:3 ingest keeps the follow set current — exactly the pattern the
//!    sibling `FollowListProjection` already uses).
//! 6. Registers an `on_change` callback that resets the engine on every
//!    follow-set perspective change.
//! 7. Registers the follow-set notifier on `NmpApp`'s identity-change observer
//!    seam so sign-in, switch, logout, and reset are pushed after the actor has
//!    written the active-account slot.
//!
//! # CRITICAL DECISION — declaration here, no static follow snapshots
//!
//! This composition root owns the app-level feed declaration: primary content
//! kinds from the active account's reactive follows perspective. It does not
//! compute or pass a static follow list. The active-follow producer and kernel
//! subscription machinery react to active-account kind:3 replacement, account
//! switch, logout, mute/block, delete, and replacement changes using the stored
//! acquisition kinds. The OP-feed engine observes resulting events through the
//! kernel's `KernelEventObserver` fan-out and clears/regrows its visible window
//! on perspective changes.
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
//! # Perspective changes reset the feed
//!
//! `ActiveFollowSet::on_change` fires on active-account kind:3 replacement,
//! account switch, and logout. All of those are feed-perspective changes: the
//! user has changed who can cause rows to appear. The engine therefore resets
//! immediately instead of letting stale rows D5-evict naturally. Re-population
//! comes from the same reactive acquisition/cache-serve path that registered
//! the new follow-feed interest.
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
//! observer repopulates the set and fires `on_change` again; the callback
//! resets the empty interim window so the new perspective is populated only by
//! B's qualifying rows. The clear-then-repopulate ordering means the
//! switch-before-kind:3 window never rebuilds against a stale follow set.
//!
//! [`ActiveAccountSlot`]: nmp_core::slots::ActiveAccountSlot

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{empty_suppression_lookup, KernelEvent, SuppressionLookup};
use nmp_core::KernelEventObserver;
use nmp_feed::{ClosureInterestShape, FeedAdvance, FeedApply, FeedController, PullFeedController};
use nmp_ffi::NmpApp;
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::op_feed::{op_feed_observer, register_op_feed, FeedEmissionState, FrameIdentity};
use nmp_nip01::OpFeedEngine;
use nmp_nip02::ActiveFollowSet;
use nmp_nip51::MuteListProjection;
use nmp_planner::InterestShape;

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
    /// The registered feed controller under `"nmp.feed.home"`.
    ///
    /// Perspective-change producers call this controller's `reset` path so the
    /// visible OP-feed state and the seq pull cursor move together.
    pub controller: Arc<dyn FeedController>,
    /// The follow-set producer — already wired as a `KernelEventObserver` for
    /// kind:3 updates and as an `NmpApp` identity observer for sign-in, switch,
    /// logout, and reset.
    pub follow_set: Arc<ActiveFollowSet>,
    /// #1740 step 2 — the observer ids the wiring installed so a feed SESSION can
    /// revoke them on `close_feed` (was app-lifetime). `[follow_set, engine]`; a
    /// zero id (poisoned slot at install) revokes as a harmless no-op.
    pub observer_ids: [nmp_core::KernelEventObserverId; 2],
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
/// `on_change` callback that resets the engine on any follow-set perspective
/// change.
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
/// This function declares active-follows acquisition from app primary kinds,
/// but never passes concrete follow pubkeys or registers a second home-feed
/// projection. See the module docs.
///
/// # Ordering
///
/// Like [`crate::register_defaults`], call before `nmp_app_start`: the engine
/// and the follow-set observer must be visible to the kernel when the first
/// event arrives.
pub fn register_op_feed_defaults(
    app: &NmpApp,
    viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
) -> OpFeedDefaults {
    register_op_feed_defaults_inner(app, viewer, primary_feed_kinds, empty_suppression_lookup())
}

/// Wire the OP-centric home feed with the default NIP-51 mute read model.
///
/// Uses the same `MuteListProjection` installed by [`crate::register_defaults`]
/// and resets the current feed window whenever the active account's mute list
/// replacement changes.
pub fn register_op_feed_defaults_with_mute(
    app: &NmpApp,
    viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
    mute: Arc<MuteListProjection>,
) -> OpFeedDefaults {
    let suppression: Arc<dyn SuppressionLookup> = mute.clone();
    let defaults = register_op_feed_defaults_inner(app, viewer, primary_feed_kinds, suppression);
    let controller_for_mute = defaults.controller.clone();
    mute.on_change(Box::new(move || {
        let _ = controller_for_mute.reset();
    }));
    defaults
}

fn register_op_feed_defaults_inner(
    app: &NmpApp,
    viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
    suppression: Arc<dyn SuppressionLookup>,
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
    // #1740 step 2: capture the id so a feed session can revoke this observer on
    // close (was discarded — app-lifetime). Zero id ⇒ soft-fail, revoke no-ops.
    let follow_set_observer_id = app.register_event_observer(follow_set_observer);

    // ── 2. Declare active-follows acquisition from app primary kinds ─────
    //
    // The app/defaults layer owns the primary-kind declaration. The kernel
    // stores only the adapter-derived acquisition kinds and uses the active
    // account's current kind:3 as a reactive perspective; concrete follow
    // pubkeys are never passed through this API.
    let _declared = app.declare_active_follows_feed(primary_feed_kinds.iter().copied());

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
    let event_lookup_for_observer = event_lookup.clone();

    // ── 4. Construct the engine ──────────────────────────────────────────
    let engine = register_op_feed(viewer.clone(), follow_set.predicate(), event_lookup);

    // ── 5. Register the engine (ingest + output) ─────────────────────────
    let observer = op_feed_observer(engine.clone(), event_lookup_for_observer, suppression);
    let observer_for_registry: Arc<dyn KernelEventObserver> = observer.clone();
    // #1740 step 2: capture so a session can revoke the engine ingest observer.
    let engine_observer_id = app.register_event_observer(observer_for_registry);

    // ── 5a. Wire the home feed to the seq-ordered pull pager (ADR-0058 §8 6B) ──
    //
    // Pull uses the same live active-follows shape as acquisition, the in-process
    // event-store scan, and the suppression/delete-aware observer used by relay
    // fan-out. `advance` only grows the render viewport after visible progress.
    let provider: Arc<dyn nmp_feed::FeedInterestShape + Send + Sync> = {
        let follow_set = follow_set.clone();
        // Capture the live active-account slot so logout/switch fail closed and
        // account changes work without re-registering the controller.
        let account_slot = active_account_slot.clone();
        // Invalid app-declared primary kinds (for example `6` or `16`) fail
        // closed: no acquisition shape, no broad scan.
        let kinds: BTreeSet<u32> =
            nmp_nip18::try_acquisition_kinds_for_primary(primary_feed_kinds.iter().copied())
                .unwrap_or_default();
        Arc::new(ClosureInterestShape::new(move || {
            live_active_follows_shape(&account_slot, &follow_set, &kinds)
        }))
    };
    let pull = app.feed_pull_fn();
    let apply: FeedApply = {
        let observer = Arc::clone(&observer);
        let engine = Arc::clone(&engine);
        Arc::new(move |event: &KernelEvent| {
            let before = visible_op_feed_payload(&engine);
            observer.on_kernel_event(event);
            visible_op_feed_payload(&engine) != before
        })
    };
    let advance: FeedAdvance = {
        let engine = Arc::clone(&engine);
        Arc::new(move || {
            engine.grow_visible_window();
        })
    };
    let reset: nmp_feed::FeedReset = {
        let engine = Arc::clone(&engine);
        Arc::new(move || {
            let had_rows = !engine.snapshot_current_window().cards.is_empty();
            engine.reset_for_perspective_change();
            had_rows
        })
    };
    // Register unconditionally; the provider re-reads live shape on load_older.
    let controller: Arc<dyn FeedController> =
        PullFeedController::new_with_perspective(provider, pull, apply, None, Some(reset), advance);
    app.register_feed(nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY, controller.clone());

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
                file_identifier: String::from_utf8_lossy(
                    nmp_nip01::op_feed::OP_FEED_FILE_IDENTIFIER,
                )
                .into_owned(),
                payload,
                projection_rev,
                ..Default::default()
            }),
        }
    });

    // ── 5. Perspective reset ─────────────────────────────────────────────
    //
    // `on_change` fires on active-account kind:3 replacement, account switch,
    // and logout. Each changes the app's perspective on which authors can make
    // rows appear, so the current feed window is invalid and must clear
    // immediately. The acquisition/cache-serve path then repopulates rows that
    // still qualify under the new perspective.
    //
    // R6-S1: this callback no longer touches the typed-projection emission state.
    // Account switches bump the kernel's `snapshot_epoch` (identity_state.rs →
    // `bump_epoch`), which the kernel publishes into the frame-identity handles
    // the typed closure reads. Follow-list changes emit new feed bytes because
    // the engine state changes.
    let controller_for_cb = controller.clone();
    follow_set.on_change(Box::new(move || {
        let _ = controller_for_cb.reset();
    }));

    let follow_set_for_identity = follow_set.clone();
    // Identity changes are pushed from `NmpApp` after the actor has written the
    // active-account slot. This is the canonical app/FFI composition seam for
    // OP-feed account reset; hosts do not call `notify_account_changed` manually.
    app.register_identity_change_observer(move |_| {
        follow_set_for_identity.notify_account_changed();
    });

    OpFeedDefaults {
        engine,
        controller,
        follow_set,
        observer_ids: [follow_set_observer_id, engine_observer_id],
    }
}

/// Build the LIVE active-follows pull [`InterestShape`], or `None` to fail closed.
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
fn live_active_follows_shape(
    account_slot: &ActiveAccountSlot,
    follow_set: &ActiveFollowSet,
    kinds: &BTreeSet<u32>,
) -> Option<InterestShape> {
    if kinds.is_empty() {
        return None; // host declared no active-follows kinds => fail closed
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
pub(crate) fn read_active(slot: &ActiveAccountSlot) -> Option<String> {
    match slot.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    }
}

fn visible_op_feed_payload(engine: &OpFeedEngine) -> Vec<u8> {
    let snapshot = engine.snapshot_current_window();
    nmp_nip01::op_feed::encode_op_feed_snapshot(&snapshot)
}

// #1740 step 2 — `FeedParams` → existing-registration compiler (sibling module).
mod session_compile;
pub use session_compile::compile_feed_params;

#[cfg(test)]
#[path = "op_feed_defaults/tests.rs"]
mod tests;
