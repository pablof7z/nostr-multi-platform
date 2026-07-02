//! `open_active_follows_op_feed` — the V-80 rung 6 (Stage 5) composition root
//! that wires the OP-centric feed renderer together.
//!
//! This is the one place in the system that names `NmpApp` (native runtime) and
//! the note-feed OP instance (`nmp-note-feed`) in the same breath. Every lower
//! layer deliberately avoids that edge: `nmp-feed` is generic, `nmp-nip01` owns
//! only kind:1/NIP-10 facts, and `nmp-nip02`'s `ActiveFollowSet` takes an
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
//!    * **item builder** — supplied inside `nmp-note-feed` itself
//!      (`NoteFeedItem::from_event_for_op_feed`).
//! 3. Registers the returned `Arc<OpFeedEngine>` as an
//!    [`ObservedProjectionSink`](nmp_core::ObservedProjectionSink) (ingest) and a
//!    [`FeedController`](nmp_feed::FeedController) under the caller's
//!    projection key (output).
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
//! V-83 added [`NmpApp::event_by_id`](crate::NmpApp::event_by_id) over the
//! kernel's published `EventStore` handle (the actor publishes
//! `Kernel::event_store_handle()` into a shared slot right after kernel
//! construction and re-publishes on `Reset`). The closure here
//! captures [`NmpApp::event_store_handle`](crate::NmpApp::event_store_handle)
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
//! `ActiveFollowSet::new` needs the kernel's [`ActiveAccountSlot`] plus a
//! store-derived latest-kind:3 reader. `open_active_follows_op_feed` reads both
//! directly from [`NmpApp::active_account_handle`](crate::NmpApp::active_account_handle)
//! and [`NmpApp::event_store_handle`](crate::NmpApp::event_store_handle), so
//! the follow predicate and identity-change observer derive from the same
//! event store the actor writes.
//!
//! # Perspective changes reset the feed
//!
//! `ActiveFollowSet` emits graph source effects on active-account kind:3
//! replacement, account switch, and logout. All of those are feed-perspective
//! changes: the user has changed who can cause rows to appear. The active-
//! follows session effect therefore reconciles observed projections, replaces
//! dependent acquisition, and resets the engine immediately instead of letting
//! stale rows D5-evict naturally. Re-population comes from the same
//! ReducedSource acquisition/cache-serve path that materializes the current
//! active-account source.
//!
//! ## The account-change race (rung-4 flagged this)
//!
//! On a switch A → B the actor updates the active-account slot, emits a state
//! frame, and `NmpApp`'s update listener fires its identity observers before
//! forwarding that frame to native. The callback registered here calls
//! `notify_account_changed()`: `ActiveFollowSet` clears A's set, hydrates B's
//! latest kind:3 follow set from the event store if one is already present,
//! re-seeds self-inclusion of B, and emits a source effect. The session effect
//! sees `B != A`, reconciles acquisition/projection state, resets the engine,
//! and records B. When B's kind:3 later ingests, `ActiveFollowSet`'s own
//! observer rebuilds from the event and emits another source effect. The
//! clear-then-hydrate ordering means the switch-before-kind:3 window never
//! rebuilds against A's stale follow set, and sign-in-prepopulated follows can
//! qualify rows immediately.
//!
//! [`ActiveAccountSlot`]: nmp_core::slots::ActiveAccountSlot

use std::sync::Arc;

use crate::{FeedOpenError, NmpApp};
use nmp_core::substrate::{empty_suppression_lookup, SuppressionLookup};
use nmp_feed::{
    FeedAdmission, FeedController, FeedHandle, FeedParams, FeedRanking, FeedRender, FeedScope,
    FeedWindow, ProjectionKey,
};
use nmp_nip02::{ActiveFollowSet, LatestKind3FollowSet};
use nmp_nip51::MuteListProjection;
use nmp_note_feed::OpFeedEngine;

#[cfg(test)]
mod active_shape;
#[cfg(test)]
use active_shape::live_active_follows_shape;

#[cfg(test)]
use nmp_core::slots::ActiveAccountSlot;

type Pubkey = String;

/// What [`open_active_follows_op_feed`] hands back to the composition caller.
pub struct ActiveFollowsOpFeedSession {
    /// The ordinary feed-session handle for the caller-owned projection.
    ///
    /// `None` means the typed declaration failed closed before registration
    /// (for example because `primary_feed_kinds` named a derived wrapper kind).
    pub handle: Option<FeedHandle>,
    /// Diagnostic handle to the session-owned OP-feed engine.
    pub engine: Arc<OpFeedEngine>,
    /// Diagnostic handle to the session-owned feed controller.
    pub controller: Arc<dyn FeedController>,
    /// Diagnostic handle to the session-owned active follow-set resolver.
    pub follow_set: Arc<ActiveFollowSet>,
}

struct NoopFeedController;

impl FeedController for NoopFeedController {
    fn load_older(&self) -> bool {
        false
    }
}

/// Wire an OP-centric active-follows feed session into `app`.
///
/// Builds active-follows [`FeedParams`] and opens them through the ordinary
/// typed feed-session compiler. The session engine owns the OP engine,
/// follow-set resolver, observed projection, pull controller, typed sidecar,
/// and teardown recipe under the caller's projection key.
///
/// # Ordering
///
/// Call before `nmp_app_start`: the engine and the follow-set observer must be
/// visible to the kernel when the first event arrives.
pub fn open_active_follows_op_feed(
    app: &NmpApp,
    viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
    projection: ProjectionKey,
) -> ActiveFollowsOpFeedSession {
    open_active_follows_op_feed_inner(
        app,
        viewer,
        primary_feed_kinds,
        projection,
        empty_suppression_lookup(),
    )
}

/// Wire an OP-centric active-follows feed with the NIP-51 mute read model.
///
/// Uses the caller-supplied `MuteListProjection` and resets the current feed
/// window whenever the active account's mute list replacement changes.
pub fn open_active_follows_op_feed_with_mute(
    app: &NmpApp,
    viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
    projection: ProjectionKey,
    mute: Arc<MuteListProjection>,
) -> ActiveFollowsOpFeedSession {
    let suppression: Arc<dyn SuppressionLookup> = mute.clone();
    let session = open_active_follows_op_feed_inner(
        app,
        viewer,
        primary_feed_kinds,
        projection.clone(),
        suppression,
    );
    if session.handle.is_some() {
        let registry = app.feed_registry_handle();
        let sender = app.command_sender();
        let projection_key = projection.as_str().to_string();
        mute.on_change(Box::new(move || {
            if registry.reset(&projection_key) {
                sender.mark_changed_since_emit();
            }
        }));
    }
    session
}

fn open_active_follows_op_feed_inner(
    app: &NmpApp,
    _viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
    projection: ProjectionKey,
    suppression: Arc<dyn SuppressionLookup>,
) -> ActiveFollowsOpFeedSession {
    let params = active_follows_op_feed_params(primary_feed_kinds, projection);
    let compiler = move |app: &NmpApp,
                         params: &FeedParams,
                         kinds: &std::collections::BTreeSet<u32>|
          -> Result<
        (
            nmp_feed::FeedSessionBuild,
            nmp_feed_session::OpScopeSessionArtifacts,
        ),
        FeedOpenError,
    > {
        let detailed = nmp_feed_session::compile_feed_params_with_suppression_and_artifacts(
            app,
            params,
            kinds,
            Arc::clone(&suppression),
        )?;
        let Some(artifacts) = detailed.artifacts else {
            return Err(FeedOpenError::ScopeNotSupportedYet {
                scope: "active-follows-op-feed-artifacts",
            });
        };
        Ok((detailed.build, artifacts))
    };
    match app.open_feed_with_output(&params, compiler) {
        Ok((handle, artifacts)) => ActiveFollowsOpFeedSession {
            handle: Some(handle),
            engine: artifacts.engine,
            controller: artifacts.controller,
            follow_set: artifacts
                .follow_set
                .unwrap_or_else(|| fallback_follow_set(app)),
        },
        Err(_) => fallback_session(app),
    }
}

#[must_use]
pub fn active_follows_op_feed_params(
    primary_feed_kinds: Vec<u32>,
    projection: ProjectionKey,
) -> FeedParams {
    FeedParams {
        primary_kinds: primary_feed_kinds,
        render: FeedRender::OpCentric,
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow {
            initial_limit: nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        },
        projection,
    }
}

fn fallback_session(app: &NmpApp) -> ActiveFollowsOpFeedSession {
    let follow_set = fallback_follow_set(app);
    let event_store = app.event_store_handle();
    let event_lookup: nmp_feed::EventLookup =
        Arc::new(move |id| nmp_core::slots::event_by_id_from_store(&event_store, id));
    let engine = nmp_note_feed::op_feed::register_op_feed(
        String::new(),
        follow_set.predicate(),
        event_lookup,
    );
    ActiveFollowsOpFeedSession {
        handle: None,
        engine,
        controller: Arc::new(NoopFeedController),
        follow_set,
    }
}

fn fallback_follow_set(app: &NmpApp) -> Arc<ActiveFollowSet> {
    ActiveFollowSet::new(
        app.active_account_handle(),
        LatestKind3FollowSet::new(app.event_store_handle()),
    )
}

#[cfg(test)]
#[path = "op_feed_session/tests.rs"]
mod tests;
