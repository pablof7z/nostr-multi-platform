//! The generalized session-engine builder for non-default feed scopes (#1740
//! step 3).
//!
//! Every [`nmp_feed::FeedScope`] that is NOT `ActiveUserFollows` (which keeps
//! the framework-default home wiring) compiles through here. The builder is a
//! SESSION WRAPPER over the existing OP-feed mechanics — the same generic engine
//! [`nmp_nip01::op_feed::register_op_feed`] the home feed uses — parameterized
//! on:
//!
//! * a COMPILED, EVENT-AWARE admission predicate (the engine's
//!   [`nmp_feed::RootAdmission`], built INSIDE the framework from resolved
//!   pubkey-set DATA / `#t` tag terms — no app closure crosses the seam) that
//!   gates which roots ENTER the feed (#1740 step 3); and
//! * a set of INTERNAL acquisition interests, registered via the kernel's
//!   dependent-interest owner under the session's projection key and withdrawn
//!   symmetrically on close.
//!
//! The session registers under the caller's UNIQUE [`nmp_feed::ProjectionKey`]
//! (not the home `OP_FEED_SNAPSHOT_KEY`), so many scope sessions coexist. Close
//! tears everything down in reverse order: withdraw each interest, remove the
//! controller + projection, revoke the ingest observer + any resolver observers.
//!
//! Doctrine map:
//! - D0: this names no app product — it consumes a compiled predicate + typed
//!   acquisition shapes. The scope→predicate semantics live in `resolve.rs`.
//! - D4: reuses `register_op_feed` + `op_feed_observer` + the kernel
//!   dependent-interest owner; no second feed engine, no re-derived filter on
//!   close.
//! - D8: each session's interests are withdrawn on close (symmetric teardown).

use std::sync::Arc;

use nmp_core::actor::ActorCommand;
use nmp_core::actor::InterestsCommand;
use nmp_core::subs::SubOwnerKey;
use nmp_core::substrate::{empty_suppression_lookup, KernelEvent};
use nmp_core::KernelEventObserver;
use nmp_feed::{
    ClosureInterestShape, FeedAdvance, FeedApply, FeedController, FeedRender, FeedReset,
    FeedSessionBuild, PullFeedController, RootAdmission,
};
use nmp_ffi::{FeedOpenError, NmpApp};

use super::source::{acquisition_children, ExtraAcquisition, ReducedSource};

/// Build a registered feed session for a reduced non-default source and return
/// its teardown recipe.
///
/// `key` is the session's unique projection key (from `params.projection`).
/// `resolved` carries the compiled admission predicate, fixed + live
/// acquisition interests, reset hooks, and any resolver observer ids that must
/// be revoked on close.
pub(super) fn build_scope_session(
    app: &NmpApp,
    key: &str,
    render: &FeedRender,
    resolved: ReducedSource,
) -> Result<FeedSessionBuild, FeedOpenError> {
    match render {
        FeedRender::OpCentric => build_op_scope_session(app, key, resolved),
        FeedRender::Flat => build_flat_scope_session(app, key, resolved),
    }
}

fn build_op_scope_session(
    app: &NmpApp,
    key: &str,
    resolved: ReducedSource,
) -> Result<FeedSessionBuild, FeedOpenError> {
    let viewer = super::super::read_active(&app.active_account_handle()).ok_or(
        FeedOpenError::ScopeNotSupportedYet {
            scope: "scope-no-active-account",
        },
    )?;

    let ReducedSource {
        admission,
        interests,
        live_shape,
        extra_acquisition,
        reset_hooks,
        resolver_observer_ids,
        identity_observer_ids,
    } = resolved;

    // ── 1. Engine over the COMPILED, EVENT-AWARE admission predicate ──────
    //
    // The compiled predicate IS the engine's ROOT-admission gate (#1740 step 3):
    // a root whose author/tags the perspective does not admit never enters the
    // feed — the perspective filters the rendered feed itself, not merely reply
    // attribution. It is built inside the framework (from resolved DATA or a
    // live framework projection) — nothing app-supplied crosses the seam. A
    // permissive follow-attribution predicate is NOT needed here (a session's
    // attribution still flows through the engine's `follow` gate, which the home
    // path sets; sessions reuse the same observer wiring). We pass the compiled
    // perspective as BOTH so a session admits roots AND attributes replies from
    // in-scope authors only.
    let root_admission: RootAdmission = admission;
    let follow_predicate: nmp_feed::FollowPredicate = {
        let root_admission = root_admission.clone();
        Arc::new(move |pk: &str| {
            // Reply attribution: gate on the author alone (build a minimal
            // author-only event view). For author-scope perspectives this is the
            // exact membership test; tag-scope perspectives never qualify a reply
            // as attribution (a reply carrying no scope tag is correctly dropped).
            let probe = nmp_core::substrate::KernelEvent {
                id: String::new(),
                author: pk.to_string(),
                kind: 0,
                created_at: 0,
                tags: Vec::new(),
                content: String::new(),
                relay_provenance: Vec::new(),
            };
            root_admission(&probe)
        })
    };
    let event_store = app.event_store_handle();
    let event_lookup: nmp_feed::EventLookup = Arc::new(move |id: &nmp_core::substrate::EventId| {
        nmp_core::slots::event_by_id_from_store(&event_store, id)
    });
    let event_lookup_for_observer = event_lookup.clone();
    let engine = nmp_nip01::op_feed::register_op_feed_with_admission(
        viewer,
        follow_predicate,
        root_admission,
        event_lookup,
    );

    // ── 2. Ingest observer ───────────────────────────────────────────────
    let observer = nmp_nip01::op_feed::op_feed_observer(
        engine.clone(),
        event_lookup_for_observer,
        empty_suppression_lookup(),
    );
    let observer_for_registry: Arc<dyn KernelEventObserver> = observer.clone();
    let engine_observer_id = app.register_event_observer(observer_for_registry);

    // ── 3. Pull controller over the live acquisition shape ───────────────
    let provider: Arc<dyn nmp_feed::FeedInterestShape + Send + Sync> = {
        let live_shape = live_shape.clone();
        Arc::new(ClosureInterestShape::new(move || live_shape()))
    };
    let pull = app.feed_pull_fn();
    let apply: FeedApply = {
        let observer = Arc::clone(&observer);
        let engine = Arc::clone(&engine);
        Arc::new(move |event: &KernelEvent| {
            let before = visible_payload(&engine);
            observer.on_kernel_event(event);
            visible_payload(&engine) != before
        })
    };
    let advance: FeedAdvance = {
        let engine = Arc::clone(&engine);
        Arc::new(move || {
            engine.grow_visible_window();
        })
    };
    let reset: FeedReset = {
        let engine = Arc::clone(&engine);
        Arc::new(move || {
            let had_rows = !engine.snapshot_current_window().cards.is_empty();
            engine.reset_for_perspective_change();
            had_rows
        })
    };
    let controller: Arc<dyn FeedController> =
        PullFeedController::new_with_perspective(provider, pull, apply, None, Some(reset), advance);
    app.register_feed(key.to_string(), controller.clone());

    // ── 3b. Typed NOFS sidecar + feed-author auto-resolve provider, STRUCTURALLY
    //         PAIRED under the session key (ADR-0063 D7, #1671 Lane H, #1740) ────
    //
    // Mirrors the home feed's typed projection so a `NOFS`-aware host renders the
    // session's window from the typed payload (generic `Value` fallback for
    // others). Sessions emit always (no incremental-apply omit bookkeeping — a
    // session feed is short-lived; the home path owns the omit optimization).
    //
    // CRITICAL (the #1740 unblocker): route BOTH lanes through ONE
    // `FeedRenderSource` via `register_feed_render_source` — NOT the bare
    // `register_typed_snapshot_projection`. The bare path installs ONLY the typed
    // sidecar and NO feed-author provider, so the authors a session feed renders
    // would never auto-`resolve_ref` → blank avatars (the exact #1671 coverage
    // hole). Pairing here makes the provider STRUCTURAL for EVERY session-engine
    // scope (Authors/Tag/List/Wot/…): there is no path that installs the sidecar
    // without the provider, so migrating a Chirp feed onto `open_feed` cannot
    // regress avatars. The same per-tick window materialization feeds both lanes,
    // so authors-resolved == window-emitted even if a concurrent `load_older`
    // widens the window mid-tick (no 1-frame blank gap; ADR-0038). Teardown via
    // `remove_projection` / `unregister_feed` (same key) covers both lanes.
    let engine_for_typed = Arc::clone(&engine);
    let typed_key = key.to_string();
    let source =
        nmp_feed::FeedRenderSource::new(move || engine_for_typed.snapshot_current_window());
    app.register_feed_render_source(key.to_string(), source, move |snapshot| {
        Some(nmp_core::TypedProjectionData {
            key: typed_key.clone(),
            schema_id: nmp_nip01::op_feed::OP_FEED_SCHEMA_ID.to_string(),
            schema_version: nmp_nip01::op_feed::OP_FEED_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(nmp_nip01::op_feed::OP_FEED_FILE_IDENTIFIER)
                .into_owned(),
            payload: nmp_nip01::op_feed::encode_op_feed_snapshot(snapshot),
            ..Default::default()
        })
    });

    // ── 4+5. Replace dependent acquisition set + reactive re-sync ─────────
    //
    // The session owns ONE dependent-interest set keyed by its projection. Fixed
    // seed/list interests and live member timeline interests are replaced
    // together. When a source set shrinks, the kernel withdraws disappeared
    // children immediately; teardown sends the empty set. The session never
    // serializes shapes to NIP-01 JSON or tracks a private open log.
    let sender = app.command_sender();
    let owner = session_acquisition_owner(key);
    let fixed_acquisition = Arc::new(interests);
    let sync_acquisition = {
        let sender = sender.clone();
        let fixed_acquisition = Arc::clone(&fixed_acquisition);
        move |extra: &ExtraAcquisition| {
            let children = acquisition_children(&fixed_acquisition, extra);
            let _ = sender.send(ActorCommand::Interests(
                InterestsCommand::ReplaceDependentInterestSet {
                    owner,
                    children,
                    reason: "feed-session-acquisition".to_string(),
                },
            ));
        }
    };
    sync_acquisition(&extra_acquisition);

    // Wire each underlying-set change to (a) re-sync acquisition for the new
    // members, then (b) reset the window so it regrows under the new perspective.
    for hook in reset_hooks {
        let controller_for_reset = controller.clone();
        let extra = extra_acquisition.clone();
        let sync_acquisition = sync_acquisition.clone();
        let notify = sender.clone();
        let reset_trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            sync_acquisition(&extra);
            let reset = controller_for_reset.reset();
            let replayed = controller_for_reset.load_older();
            if reset || replayed {
                notify.mark_changed_since_emit();
            }
        });
        hook(reset_trigger);
    }

    // ── 6. Teardown recipe (registration order = reverse of execution) ───
    //
    // Execution order on close (the registry reverses the Vec):
    //   1. unregister the controller            (registry removal, runs first)
    //   2. revoke the engine ingest observer
    //   3. revoke each resolver observer
    //   4. remove the projection
    //   5. clear acquisition set                (WITHDRAW actor-owned state)
    //   6. mark-changed                          (the notification, runs last)
    let teardown_handle = app.feed_teardown();
    let mut teardown: Vec<nmp_feed::TeardownAction> = Vec::new();
    teardown.push(teardown_handle.mark_changed()); // exec #6 (last)
    teardown.push(clear_acquisition_set(sender.clone(), owner)); // exec #5
    teardown.push(teardown_handle.remove_projection(key.to_string())); // exec #4
    for id in &resolver_observer_ids {
        teardown.push(teardown_handle.revoke_observer(*id));
    } // exec #3
    for id in &identity_observer_ids {
        teardown.push(teardown_handle.revoke_identity_observer(*id));
    } // exec #3
    teardown.push(teardown_handle.revoke_observer(engine_observer_id)); // exec #2
    teardown.push(teardown_handle.unregister_feed(key.to_string())); // exec #1 (first)

    Ok(FeedSessionBuild {
        projection_key: nmp_feed::ProjectionKey(key.to_string()),
        teardown,
    })
}

fn build_flat_scope_session(
    app: &NmpApp,
    key: &str,
    resolved: ReducedSource,
) -> Result<FeedSessionBuild, FeedOpenError> {
    let ReducedSource {
        admission,
        interests,
        live_shape,
        extra_acquisition,
        reset_hooks,
        resolver_observer_ids,
        identity_observer_ids,
    } = resolved;

    let feed = nmp_nip01::FlatFeed::new(admission);
    let observer_for_registry: Arc<dyn KernelEventObserver> = feed.clone();
    let engine_observer_id = app.register_event_observer(observer_for_registry);

    let provider: Arc<dyn nmp_feed::FeedInterestShape + Send + Sync> = {
        let live_shape = live_shape.clone();
        Arc::new(ClosureInterestShape::new(move || live_shape()))
    };
    let pull = app.feed_pull_fn();
    let apply: FeedApply = {
        let feed = Arc::clone(&feed);
        Arc::new(move |event: &KernelEvent| {
            let before = visible_flat_payload(&feed);
            feed.on_kernel_event(event);
            visible_flat_payload(&feed) != before
        })
    };
    let advance: FeedAdvance = {
        let feed = Arc::clone(&feed);
        Arc::new(move || {
            feed.grow_visible_window();
        })
    };
    let reset: FeedReset = {
        let feed = Arc::clone(&feed);
        Arc::new(move || feed.reset_for_perspective_change())
    };
    let controller: Arc<dyn FeedController> =
        PullFeedController::new_with_perspective(provider, pull, apply, None, Some(reset), advance);
    app.register_feed(key.to_string(), controller.clone());

    let feed_for_typed = Arc::clone(&feed);
    let typed_key = key.to_string();
    let source = nmp_feed::FeedRenderSource::new(move || feed_for_typed.snapshot_current_window());
    app.register_feed_render_source(key.to_string(), source, move |snapshot| {
        Some(nmp_core::TypedProjectionData {
            key: typed_key.clone(),
            schema_id: nmp_nip01::op_feed::OP_FEED_SCHEMA_ID.to_string(),
            schema_version: nmp_nip01::op_feed::OP_FEED_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(nmp_nip01::op_feed::OP_FEED_FILE_IDENTIFIER)
                .into_owned(),
            payload: nmp_nip01::op_feed::encode_op_feed_snapshot(snapshot),
            ..Default::default()
        })
    });
    let replayed_tail = app.load_older_feed(key);
    let replayed_ids = super::flat_replay::replay_fixed_event_ids(app, &feed, &interests);
    if replayed_ids && !replayed_tail {
        (app.feed_teardown().mark_changed())();
    }

    let sender = app.command_sender();
    let owner = session_acquisition_owner(key);
    let fixed_acquisition = Arc::new(interests);
    let sync_acquisition = {
        let sender = sender.clone();
        let fixed_acquisition = Arc::clone(&fixed_acquisition);
        move |extra: &ExtraAcquisition| {
            let children = acquisition_children(&fixed_acquisition, extra);
            let _ = sender.send(ActorCommand::Interests(
                InterestsCommand::ReplaceDependentInterestSet {
                    owner,
                    children,
                    reason: "feed-session-acquisition".to_string(),
                },
            ));
        }
    };
    sync_acquisition(&extra_acquisition);
    for hook in reset_hooks {
        let controller_for_reset = controller.clone();
        let extra = extra_acquisition.clone();
        let sync_acquisition = sync_acquisition.clone();
        let notify = sender.clone();
        let reset_trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            sync_acquisition(&extra);
            let reset = controller_for_reset.reset();
            let replayed = controller_for_reset.load_older();
            if reset || replayed {
                notify.mark_changed_since_emit();
            }
        });
        hook(reset_trigger);
    }

    let teardown_handle = app.feed_teardown();
    let mut teardown: Vec<nmp_feed::TeardownAction> = Vec::new();
    teardown.push(teardown_handle.mark_changed());
    teardown.push(clear_acquisition_set(sender.clone(), owner));
    teardown.push(teardown_handle.remove_projection(key.to_string()));
    for id in &resolver_observer_ids {
        teardown.push(teardown_handle.revoke_observer(*id));
    }
    for id in &identity_observer_ids {
        teardown.push(teardown_handle.revoke_identity_observer(*id));
    }
    teardown.push(teardown_handle.revoke_observer(engine_observer_id));
    teardown.push(teardown_handle.unregister_feed(key.to_string()));

    Ok(FeedSessionBuild {
        projection_key: nmp_feed::ProjectionKey(key.to_string()),
        teardown,
    })
}

fn visible_payload(engine: &nmp_nip01::OpFeedEngine) -> Vec<u8> {
    let snapshot = engine.snapshot_current_window();
    nmp_nip01::op_feed::encode_op_feed_snapshot(&snapshot)
}

fn visible_flat_payload(feed: &nmp_nip01::FlatFeed) -> Vec<u8> {
    let snapshot = feed.snapshot_current_window();
    nmp_nip01::op_feed::encode_op_feed_snapshot(&snapshot)
}

fn session_acquisition_owner(key: &str) -> SubOwnerKey {
    SubOwnerKey::new(("feed-session-acquisition", key))
}

fn clear_acquisition_set(
    sender: nmp_core::CommandSender,
    owner: SubOwnerKey,
) -> nmp_feed::TeardownAction {
    Box::new(move || {
        let _ = sender.send(ActorCommand::Interests(
            InterestsCommand::ReplaceDependentInterestSet {
                owner,
                children: Vec::new(),
                reason: "feed-session-acquisition-close".to_string(),
            },
        ));
    })
}
