//! `register_op_feed_defaults` — the V-80 rung 6 (Stage 5) composition root
//! that wires the OP-centric feed renderer together.
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
//! 2. Builds the three inputs `register_op_feed` needs:
//!    * **follow predicate** — `active_follow_set.predicate()` (live view of
//!      the active account's follow set);
//!    * **event lookup** — a synchronous read through the kernel event-store
//!      handle exposed by `NmpApp`; this is a local cache read only, never
//!      acquisition;
//!    * **card builder** — supplied inside `register_op_feed` itself
//!      (`TimelineEventCard::from_event_for_op_feed`).
//! 3. Registers the returned `Arc<OpFeedEngine>` as an
//!    [`ObservedProjectionSink`](nmp_core::ObservedProjectionSink) (ingest) and a
//!    [`FeedController`](nmp_feed::FeedController) under
//!    `"nmp.feed.home"` (output).
//! 4. Registers the `ActiveFollowSet` as its own `ObservedProjectionSink` (so
//!    kind:3 ingest keeps the follow set current — exactly the pattern the
//!    sibling `FollowListProjection` already uses).
//! 5. Registers an `on_change` callback that resets the engine on every
//!    follow-set perspective change.
//! 6. Registers the follow-set notifier on `NmpApp`'s identity-change observer
//!    seam so sign-in, switch, logout, and reset are pushed after the actor has
//!    written the active-account slot.
//!
//! # CRITICAL DECISION — declaration here, no static follow snapshots
//!
//! Feed acquisition is owned by the `open_feed(FeedScope::ActiveUserFollows)`
//! reduced-source path. This composition helper wires only the OP-feed render
//! engine and its live follow predicate; it never declares actor-owned
//! acquisition.
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
//! a read from a `ObservedProjectionSink` callback (actor thread) sees the
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
//! comes from the same ReducedSource acquisition/cache-serve path that
//! materializes the current active-account source.
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

use nmp_core::substrate::{
    empty_suppression_lookup, KernelEvent, ObservedProjectionRegistrar, SuppressionLookup,
};
use nmp_core::ObservedProjectionSink;
use nmp_feed::{ClosureInterestShape, FeedAdvance, FeedApply, FeedController, PullFeedController};
use nmp_ffi::NmpApp;
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::op_feed::{op_feed_observer, register_op_feed, FeedEmissionState, FrameIdentity};
use nmp_nip01::OpFeedEngine;
use nmp_nip02::ActiveFollowSet;
use nmp_nip51::MuteListProjection;
use nmp_planner::InterestShape;

mod active_shape;
use crate::runtimes::active_observed_projection::ActiveObservedProjection;
mod dynamic_observer;
use active_shape::{live_active_follows_shape, read_active};

#[cfg(test)]
use nmp_core::slots::ActiveAccountSlot;

/// What [`register_op_feed_defaults`] hands back to the composition caller.
///
/// Returns both registered pieces so tests and diagnostic callers can inspect
/// the engine and the follow-set producer. Production identity changes are
/// driven through the `NmpApp` observer registered inside
/// [`register_op_feed_defaults`]; callers do not manually notify the follow set.
pub struct OpFeedDefaults {
    /// The registered OP-feed engine — already wired as a `ObservedProjectionSink`
    /// (ingest) and a `FeedController` under `"nmp.feed.home"` (output).
    pub engine: Arc<OpFeedEngine>,
    /// The registered feed controller under `"nmp.feed.home"`.
    ///
    /// Perspective-change producers call this controller's `reset` path so the
    /// visible OP-feed state and the seq pull cursor move together.
    pub controller: Arc<dyn FeedController>,
    /// The follow-set producer — already wired as a `ObservedProjectionSink` for
    /// kind:3 updates and as an `NmpApp` identity observer for sign-in, switch,
    /// logout, and reset.
    pub follow_set: Arc<ActiveFollowSet>,
    /// Diagnostic observer ids for `[follow_set, engine]` after initial sync.
    /// The follow-set observer is active-account dynamic, so it is `0` until a
    /// signed-in account opens the concrete `authors=[active]` observer.
    pub observer_ids: [nmp_core::ObservedProjectionId; 2],
}

/// Wire the OP-centric home feed into `app`.
///
/// Constructs the [`nmp_nip02::ActiveFollowSet`] over the app's active-account slot,
/// builds the engine, and registers it as both an [`ObservedProjectionSink`] and
/// a [`FeedController`] under `"nmp.feed.home"`. Also registers a typed
/// `NOFS` sidecar projection under the same key (ADR-0038 T1) ALONGSIDE the
/// generic `Value` `FeedController` — a host with a `NOFS` decoder prefers the
/// typed payload, others fall back to the generic `Value` subtree. Finally
/// registers the `ActiveFollowSet` as its own `ObservedProjectionSink` and an
/// `on_change` callback that resets on any follow-set perspective change.
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
/// This function wires render state only. `open_feed(FeedScope::ActiveUserFollows)`
/// owns acquisition through the reduced-source/dependent-interest path. See the
/// module docs.
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

    // Register the follow-set as its own active observed projection so the
    // active account's kind:3 ingest keeps the set current. No observer opens
    // before sign-in; once the active pubkey is known the observer is opened
    // with `authors=[active] / kinds=[3]`, replaying matching cached events
    // before live activation.
    let follow_set_observer: Arc<dyn ObservedProjectionSink> = follow_set.clone();
    let follow_set_observer = Arc::new(ActiveObservedProjection::new(
        active_account_slot.clone(),
        app.observed_projection_registrar_handle(),
        follow_set_observer,
        "nmp.feed.home.follow_set",
        1,
        64,
        Arc::new(|pubkey| nmp_planner::InterestShape {
            kinds: [nmp_kinds::KIND_CONTACT_LIST].into_iter().collect(),
            authors: [pubkey.to_string()].into_iter().collect(),
            ..Default::default()
        }),
    ));
    let follow_set_observer_tick = Arc::clone(&follow_set_observer);
    app.register_snapshot_tick_observer(move || follow_set_observer_tick.sync());

    // ── 2. Event lookup (V-83 — real synchronous kernel event read) ──────
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

    // ── 3. Construct the engine ──────────────────────────────────────────
    let engine = register_op_feed(viewer.clone(), follow_set.predicate(), event_lookup);

    // ── 4. Register the engine (ingest + output) ─────────────────────────
    let observer = op_feed_observer(engine.clone(), event_lookup_for_observer, suppression);

    // ── 4a. Wire the home feed to the seq-ordered pull pager (ADR-0058 §8 6B) ──
    //
    // Pull uses the same live active-follows shape as acquisition, the in-process
    // event-store scan, and the suppression/delete-aware observer used by relay
    // fan-out. `advance` only grows the render viewport after visible progress.
    let live_shape: Arc<dyn Fn() -> Option<InterestShape> + Send + Sync> = {
        let follow_set = follow_set.clone();
        // Capture the live active-account slot so logout/switch fail closed and
        // account changes work without re-registering the controller.
        let account_slot = active_account_slot.clone();
        // Invalid app-declared primary kinds (for example `6` or `16`) fail
        // closed: no acquisition shape, no broad scan.
        let kinds: BTreeSet<u32> =
            nmp_nip18::try_acquisition_kinds_for_primary(primary_feed_kinds.iter().copied())
                .unwrap_or_default();
        Arc::new(move || live_active_follows_shape(&account_slot, &follow_set, &kinds))
    };
    let provider: Arc<dyn nmp_feed::FeedInterestShape + Send + Sync> = {
        let live_shape = live_shape.clone();
        Arc::new(ClosureInterestShape::new(move || live_shape()))
    };
    let observer_for_registry: Arc<dyn ObservedProjectionSink> = observer.clone();
    let engine_observer = dynamic_observer::DynamicObservedProjection::new(
        app.observed_projection_handle(),
        observer_for_registry,
        "nmp.feed.home.engine",
        0,
        live_shape,
        512,
    );
    engine_observer.sync();
    let engine_observer_id = engine_observer.current_id();
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

    // ── 4b. Register the typed NOFS sidecar (ADR-0038 Commitment 5) ───────
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
    // R6-S1 emission state + frame-identity rebaseline (the freeze fix): the
    // producer rebaselines on the EXACT signal the host's `ProjectionCache` resets
    // on — the frame `(session_id, snapshot_epoch)` tuple, published each tick into
    // shared `Arc<AtomicU64>` handles the closure reads lock-free. Forcing a full
    // baseline whenever either component changes covers account-switch, Reset, and
    // any future epoch-class bump with ONE durable signal (no bespoke per-event
    // epoch counter — the Reset-blind `emission_epoch` is deleted).
    let engine_for_typed = Arc::clone(&engine);
    let incremental_apply = app.incremental_apply_handle();
    let (frame_session_id, frame_snapshot_epoch) = app.frame_identity_handles();
    // The closure must be `Send + Sync` (`register_feed_render_source`), so the
    // `FeedEmissionState` is wrapped in a `Mutex` to satisfy `Sync`; the lock is
    // uncontested (only the actor thread calls the closure, under the registry's
    // own mutex).
    let emission_state = Arc::new(Mutex::new(FeedEmissionState::new(incremental_apply)));

    // ── 4b. Structural typed-sidecar + feed-author provider (ADR-0063 D7,
    //         #1671 Lane H) ────────────────────────────────────────────────────
    //
    // ONE `FeedRenderSource` materializes the home feed's window ONCE per tick and
    // feeds BOTH the typed sidecar AND the feed-author auto-resolve provider via
    // `register_feed_render_source` — structural pairing (the sidecar cannot exist
    // without the provider that resolves the authors it renders). Both lanes read
    // the SAME per-tick materialization, so authors-resolved == window-emitted even
    // if a concurrent `load_older` widens the window mid-tick (no 1-frame blank gap;
    // ADR-0038). The home feed is PERMANENT so the provider is never removed. R6-S1
    // emission-state omit + frame-identity rebaseline live in the encoder closure.
    let source =
        nmp_feed::FeedRenderSource::new(move || engine_for_typed.snapshot_current_window());
    app.register_feed_render_source(
        nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY,
        source,
        move |snapshot| {
            let payload = nmp_nip01::op_feed::encode_op_feed_snapshot(snapshot);
            // Read this tick's frame identity lock-free (the kernel published it
            // at the top of `make_update`, before this closure runs).
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
                    // The host cache retains the prior value (omit==retain).
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
        },
    );

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
    let engine_observer_for_cb = engine_observer.clone();
    follow_set.on_change(Box::new(move || {
        engine_observer_for_cb.sync();
        let _ = controller_for_cb.reset();
    }));

    let follow_set_for_identity = follow_set.clone();
    let follow_set_observer_for_identity = Arc::clone(&follow_set_observer);
    // Identity changes are pushed from `NmpApp` after the actor has written the
    // active-account slot. This is the canonical app/FFI composition seam for
    // OP-feed account reset; hosts do not call `notify_account_changed` manually.
    app.register_identity_change_observer(move |_| {
        follow_set_for_identity.notify_account_changed();
        follow_set_observer_for_identity.sync();
    });

    OpFeedDefaults {
        engine,
        controller,
        follow_set,
        observer_ids: [follow_set_observer.current_id(), engine_observer_id],
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
