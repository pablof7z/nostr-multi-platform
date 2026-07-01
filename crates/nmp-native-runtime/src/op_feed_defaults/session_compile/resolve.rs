//! `FeedScope` → [`ReducedSource`] for the perspective compiler (#1740 step 3).
//!
//! This is the ONLY module that touches the resolution snapshots — kind:3
//! follows ([`nmp_nip02::ActiveFollowSet`]), NIP-51 pubkey lists
//! ([`nmp_nip51::PeopleListProjection`] plus the kind:10000 source in
//! `nip51_sources`), and ranked WoT candidates (the #1698
//! [`nmp_wot::score::WotGraph`] query). It reuses those single-source
//! mechanisms; it never re-derives a follow graph or list parser (D4).
//!
//! Each non-default scope resolves to a [`ReducedSource`]:
//! * `admission` — the engine's EVENT-AWARE [`nmp_feed::RootAdmission`], built
//!   INSIDE the framework from a [`nmp_feed::AdmitExpr`] (static sets / `#t` tag
//!   / set algebra) OR from a LIVE framework projection (reactive scopes). No
//!   app closure crosses FFI. It gates which roots ENTER the feed (#1740 step 3),
//!   not just reply attribution.
//! * `interests` — the internal typed acquisition interests.
//! * `live_shape` — the live pull acquisition shape (re-read on `load_older`).
//! * `reset_hooks` — closures that install a window-reset on legacy reactive
//!   sources that have not moved to graph source effects yet, plus the observer
//!   ids to revoke. `ActiveUserFollows` uses graph-owned source effects.
//!
//! Deferred / fail-closed (typed `ScopeNotSupportedYet`, no registration):
//! * `RelaySet` — no framework relay-set-id resolver exists; relay-pinned
//!   acquisition is out of step-3 scope.
//! * `ContactList { owner }` for a FOREIGN owner — only the active viewer's
//!   kind:3 has a framework resolver; an arbitrary owner's contact list has no
//!   single-source projection. The active-owner case resolves via the follow set.
//! * `CustomPerspectiveId` — step 4 (the registration mechanism).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{FeedOpenError, NmpApp};
use nmp_core::substrate::{
    KernelEvent, ObservedProjection, ObservedProjectionReconciler, ObservedProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;
use nmp_feed::RootAdmission;
use nmp_kinds::KIND_FOLLOW_SET;
use nmp_planner::InterestShape;

use super::source::{
    AcquisitionInterest, ExtraAcquisition, LiveShape, OpSessionIdentity, ReducedSource, ResetHook,
};
use super::wot_graph::SessionWotGraph;

const KIND_CONTACT_LIST: u32 = 3;

/// Resolve a non-set-algebra scope. Set algebra is handled by
/// [`super::set_algebra`]; `CustomPerspectiveId` is handled in `custom.rs`.
pub(super) fn resolve_scope(
    app: &NmpApp,
    scope: &nmp_feed::FeedScope,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    use nmp_feed::FeedScope as S;
    match scope {
        S::Authors { authors } => super::resolve_static::resolve_authors(authors, kinds),
        S::ActiveUserFollows => resolve_active_user_follows(app, kinds),
        S::ContactList { owner } => resolve_contact_list(app, owner, kinds),
        S::ListMembers { list } => resolve_list_members(app, &list.0, kinds),
        S::Wot { seed, .. } => resolve_wot(app, &seed.0, kinds),
        S::Tag { term } => Ok(super::resolve_static::resolve_tag(&term.0, kinds)),
        S::Referrer { event_id } => super::resolve_static::resolve_referrer(event_id, kinds),
        S::PointerTargets {
            pointers,
            pointer_kinds,
        } => super::pointer_targets::resolve_pointer_targets(app, pointers, pointer_kinds, kinds),
        S::RelaySet { .. } => Err(not_supported("RelaySet")),
        S::Union(l, r) => super::set_algebra::resolve_set_op(app, SetOp::Union, l, r, kinds),
        S::Intersection(l, r) => {
            super::set_algebra::resolve_set_op(app, SetOp::Intersection, l, r, kinds)
        }
        S::Difference(l, r) => {
            super::set_algebra::resolve_set_op(app, SetOp::Difference, l, r, kinds)
        }
        S::CustomPerspectiveId(_) => {
            // Handled by the dispatcher; unreachable here. Fail closed.
            Err(not_supported("scope-routing"))
        }
    }
}

/// The set-algebra operator, shared with `set_algebra.rs`.
#[derive(Clone, Copy)]
pub(super) enum SetOp {
    Union,
    Intersection,
    Difference,
}

pub(super) fn not_supported(scope: &'static str) -> FeedOpenError {
    FeedOpenError::ScopeNotSupportedYet { scope }
}

// ── ContactList { owner } ────────────────────────────────────────────────

fn resolve_active_user_follows(
    app: &NmpApp,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    let initial_viewer = super::super::read_active(&app.active_account_handle());
    resolve_active_follow_set(
        app,
        kinds,
        initial_viewer,
        OpSessionIdentity::AllowMissingActive,
    )
}

/// The active viewer's contact list resolves via the framework
/// [`nmp_nip02::ActiveFollowSet`] (live, reactive). A FOREIGN owner's kind:3 has
/// no single-source framework resolver → fail closed (deferred to step 4).
fn resolve_contact_list(
    app: &NmpApp,
    owner: &str,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    let viewer = super::super::read_active(&app.active_account_handle())
        .ok_or_else(|| not_supported("ContactList-no-active-account"))?;
    if owner != viewer {
        return Err(not_supported("ContactList-foreign-owner"));
    }

    resolve_active_follow_set(app, kinds, Some(viewer), OpSessionIdentity::RequireActive)
}

fn resolve_active_follow_set(
    app: &NmpApp,
    kinds: &BTreeSet<u32>,
    initial_viewer: Option<String>,
    op_session_identity: OpSessionIdentity,
) -> Result<ReducedSource, FeedOpenError> {
    // A fresh ActiveFollowSet over the same active-account slot and the
    // kernel event store, registered as a session observer so kind:3 ingest
    // keeps the predicate live (reactive).
    let follow_set = nmp_nip02::ActiveFollowSet::new(
        app.active_account_handle(),
        nmp_nip02::LatestKind3FollowSet::new(app.event_store_handle()),
    );
    let resolver_shape_slot = app.active_account_handle();
    let resolver_live_shape: LiveShape = Arc::new(move || {
        let viewer = super::super::read_active(&resolver_shape_slot)?;
        Some(seed_contacts_shape(&viewer))
    });
    let follow_observer: Arc<dyn ObservedProjectionSink> = follow_set.clone();
    let resolver_reconciler = ObservedProjectionReconciler::new(
        app.observed_projection_registrar_handle(),
        follow_observer,
        "nmp.feed.resolver.follow_set",
        0,
        64,
        resolver_live_shape,
    );
    resolver_reconciler.sync();
    let resolver_for_identity = resolver_reconciler.clone();
    let resolver_for_teardown = resolver_reconciler.clone();
    let follow_set_for_identity = Arc::clone(&follow_set);
    let follow_set_for_replay = Arc::clone(&follow_set);
    let replay_slot = app.active_account_handle();
    let replay_pull = app.feed_pull_fn();
    let identity_observer_id = app.register_identity_change_observer(move |_| {
        follow_set_for_identity.notify_account_changed();
        resolver_for_identity.sync();
        if let Some(viewer) = super::super::read_active(&replay_slot) {
            super::source_replay::replay_source_shape_with_pull(
                &replay_pull,
                follow_set_for_replay.as_ref(),
                seed_contacts_shape(&viewer),
            );
        }
    });
    if let Some(viewer) = initial_viewer {
        super::source_replay::replay_source_shape(
            app,
            follow_set.as_ref(),
            seed_contacts_shape(&viewer),
        );
    }

    // Home/active-follow OP semantics: acquisition limits direct roots to the
    // live follow set, while attribution lets followed replies surface
    // non-followed roots. Root admission therefore stays permissive here; it is
    // not a wildcard relay demand because `live_shape` still constrains pull and
    // dependent acquisition to the active follow set.
    let follow_predicate = follow_set.predicate();
    let admission: RootAdmission = nmp_feed::admit_all_roots();
    let live_shape = follow_set_live_shape(&app.active_account_handle(), &follow_set, kinds);
    let interests = Vec::new();
    let extra_acquisition =
        active_contact_list_extra_acquisition(app.active_account_handle(), &live_shape);

    Ok(ReducedSource {
        op_session_identity,
        admission,
        attribution: follow_predicate,
        interests,
        extra_acquisition,
        live_shape,
        reset_hooks: Vec::new(),
        resolver_observer_ids: Vec::new(),
        identity_observer_ids: vec![identity_observer_id],
        resolver_teardown: vec![Box::new(move || resolver_for_teardown.close_current())],
        active_follow_set: Some(follow_set),
    })
}

// ── ListMembers { list } (NIP-51 pubkey sources) ─────────────────────────

fn resolve_list_members(
    app: &NmpApp,
    list_id: &str,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    if list_id == nmp_nip51::ACTIVE_MUTE_LIST_PUBKEY_SOURCE_ID {
        return super::nip51_sources::resolve_active_mute_list_members(app, kinds);
    }

    // The list owner is the active viewer (the projection is owner-gated). No
    // active account ⇒ fail closed (no list to resolve).
    let viewer = super::super::read_active(&app.active_account_handle())
        .ok_or_else(|| not_supported("ListMembers-no-active-account"))?;

    let projection = Arc::new(nmp_nip51::PeopleListProjection::new(
        app.active_account_handle(),
    ));
    let observer_id = app.open_observed_projection(ObservedProjection::from_shape(
        Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>,
        "nmp.feed.resolver.people_list",
        0,
        viewer_list_shape(&viewer),
        64,
    ));
    let projection_for_identity = Arc::clone(&projection);
    let projection_for_replay = Arc::clone(&projection);
    let replay_slot = app.active_account_handle();
    let replay_pull = app.feed_pull_fn();
    let identity_observer_id = app.register_identity_change_observer(move |_| {
        projection_for_identity.notify_account_changed();
        if let Some(viewer) = super::super::read_active(&replay_slot) {
            super::source_replay::replay_source_shape_with_pull(
                &replay_pull,
                projection_for_replay.as_ref(),
                viewer_list_shape(&viewer),
            );
        }
    });
    super::source_replay::replay_source_shape(app, projection.as_ref(), viewer_list_shape(&viewer));

    // LIVE predicate over the projection's current members. NIP-51 has not
    // moved to source-graph effects yet, so its projection still uses the
    // legacy reset hook below.
    let admission: RootAdmission = {
        let projection = Arc::clone(&projection);
        let list_id = list_id.to_string();
        Arc::new(move |event: &KernelEvent| projection.members(&list_id).contains(&event.author))
    };
    let attribution: nmp_feed::FollowPredicate = {
        let projection = Arc::clone(&projection);
        let list_id = list_id.to_string();
        Arc::new(move |pubkey: &str| projection.members(&list_id).contains(pubkey))
    };

    let interests = Vec::new();

    let live_shape: LiveShape = {
        let projection = Arc::clone(&projection);
        let list_id = list_id.to_string();
        let kinds = kinds.clone();
        Arc::new(move || {
            let members = projection.members(&list_id);
            if members.is_empty() || kinds.is_empty() {
                return None;
            }
            Some(InterestShape::timeline_for(
                members.into_iter().collect(),
                kinds.clone(),
            ))
        })
    };

    let reset_proj = Arc::clone(&projection);
    let reset_hook: ResetHook = Box::new(move |reset| {
        reset_proj.on_change(Box::new(move || reset()));
    });
    let extra_acquisition = list_members_extra_acquisition(
        app.active_account_handle(),
        &projection,
        list_id,
        kinds,
        &live_shape,
    );

    Ok(ReducedSource {
        op_session_identity: OpSessionIdentity::RequireActive,
        admission,
        attribution,
        interests,
        extra_acquisition,
        live_shape,
        reset_hooks: vec![reset_hook],
        resolver_observer_ids: vec![observer_id],
        identity_observer_ids: vec![identity_observer_id],
        resolver_teardown: Vec::new(),
        active_follow_set: None,
    })
}

// ── Wot { seed, rules } — reuse the #1698 ranked query ────────────────────

fn resolve_wot(
    app: &NmpApp,
    seed: &str,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    // A session-scoped WoT graph observer, reusing `WotGraph` (the #1698 ranked
    // second-degree query) — we do NOT touch the singleton bootstrap runtime.
    let graph = Arc::new(SessionWotGraph::new(seed.to_string(), KIND_CONTACT_LIST));
    let observer_id = app.open_observed_projection(ObservedProjection::from_shape(
        Arc::clone(&graph) as Arc<dyn ObservedProjectionSink>,
        "nmp.feed.resolver.wot",
        0,
        seed_contacts_shape(seed),
        256,
    ));

    let admission: RootAdmission = {
        let graph = Arc::clone(&graph);
        Arc::new(move |event: &KernelEvent| graph.admits(&event.author))
    };
    let attribution: nmp_feed::FollowPredicate = {
        let graph = Arc::clone(&graph);
        Arc::new(move |pubkey: &str| graph.admits(pubkey))
    };

    // Acquire the seed's contact list (kind:3). The seed's DIRECT follows' kind:3
    // (needed to rank second-degree candidates) and the candidates' timelines are
    // re-synced live by the session engine as the graph fills (extra_acquisition +
    // live_shape below).
    let interests = vec![AcquisitionInterest::active_account(seed_contacts_shape(
        seed,
    ))];

    let live_shape: LiveShape = {
        let graph = Arc::clone(&graph);
        let kinds = kinds.clone();
        Arc::new(move || {
            let candidates = graph.ranked_candidates();
            if candidates.is_empty() || kinds.is_empty() {
                return None;
            }
            Some(InterestShape::timeline_for(
                candidates.into_iter().collect(),
                kinds.clone(),
            ))
        })
    };

    // The second-degree ranking needs each direct follow's kind:3 contact list,
    // and the candidates' primary-kind timeline must be acquired once ranked.
    // Acquire both live as the graph fills (seed kind:3 → direct follows known →
    // fetch their kind:3 → candidates rank → fetch their timelines).
    let extra_acquisition: ExtraAcquisition = {
        let graph = Arc::clone(&graph);
        let timeline_kinds = kinds.clone();
        Arc::new(move || {
            let mut shapes = Vec::new();
            let follows = graph.direct_follows();
            if !follows.is_empty() {
                let k: BTreeSet<u32> = [KIND_CONTACT_LIST].into_iter().collect();
                shapes.push(AcquisitionInterest::active_account(
                    InterestShape::timeline_for(follows, k),
                ));
            }
            let candidates = graph.ranked_candidates();
            if !candidates.is_empty() && !timeline_kinds.is_empty() {
                shapes.push(AcquisitionInterest::active_account(
                    InterestShape::timeline_for(
                        candidates.into_iter().collect(),
                        timeline_kinds.clone(),
                    ),
                ));
            }
            shapes
        })
    };

    let reset_graph = Arc::clone(&graph);
    let reset_hook: ResetHook = Box::new(move |reset| {
        reset_graph.on_change(Box::new(move || reset()));
    });

    Ok(ReducedSource {
        op_session_identity: OpSessionIdentity::RequireActive,
        admission,
        attribution,
        interests,
        live_shape,
        extra_acquisition,
        reset_hooks: vec![reset_hook],
        resolver_observer_ids: vec![observer_id],
        identity_observer_ids: Vec::new(),
        resolver_teardown: Vec::new(),
        active_follow_set: None,
    })
}

// ── Typed acquisition shape helpers ───────────────────────────────────────

fn viewer_list_shape(viewer: &str) -> InterestShape {
    InterestShape::timeline_for(
        [viewer.to_string()].into_iter().collect(),
        [KIND_FOLLOW_SET].into_iter().collect(),
    )
}

fn seed_contacts_shape(seed: &str) -> InterestShape {
    InterestShape::timeline_for(
        [seed.to_string()].into_iter().collect(),
        [KIND_CONTACT_LIST].into_iter().collect(),
    )
}

fn follow_set_live_shape(
    slot: &nmp_core::slots::ActiveAccountSlot,
    follow_set: &Arc<nmp_nip02::ActiveFollowSet>,
    kinds: &BTreeSet<u32>,
) -> LiveShape {
    let slot = slot.clone();
    let follow_set = Arc::clone(follow_set);
    let kinds = kinds.clone();
    Arc::new(move || {
        if kinds.is_empty() {
            return None;
        }
        let viewer = super::super::read_active(&slot)?;
        let mut authors: BTreeSet<String> = follow_set.follows().into_iter().collect();
        authors.insert(viewer);
        Some(InterestShape::timeline_for(authors, kinds.clone()))
    })
}

fn active_contact_list_extra_acquisition(
    slot: nmp_core::slots::ActiveAccountSlot,
    live_shape: &LiveShape,
) -> ExtraAcquisition {
    let live_shape = Arc::clone(live_shape);
    Arc::new(move || {
        let mut shapes = Vec::new();
        if let Some(viewer) = super::super::read_active(&slot) {
            shapes.push(AcquisitionInterest::active_account(seed_contacts_shape(
                &viewer,
            )));
        }
        if let Some(shape) = live_shape() {
            shapes.push(AcquisitionInterest::active_account(shape));
        }
        shapes
    })
}

fn list_members_extra_acquisition(
    slot: nmp_core::slots::ActiveAccountSlot,
    projection: &Arc<nmp_nip51::PeopleListProjection>,
    list_id: &str,
    kinds: &BTreeSet<u32>,
    live_shape: &LiveShape,
) -> ExtraAcquisition {
    let projection = Arc::clone(projection);
    let list_id = list_id.to_string();
    let kinds = kinds.clone();
    let live_shape = Arc::clone(live_shape);
    Arc::new(move || {
        let mut shapes = Vec::new();
        if let Some(viewer) = super::super::read_active(&slot) {
            shapes.push(AcquisitionInterest::active_account(viewer_list_shape(
                &viewer,
            )));
        }
        if !projection.members(&list_id).is_empty() && !kinds.is_empty() {
            if let Some(shape) = live_shape() {
                shapes.push(AcquisitionInterest::active_account(shape));
            }
        }
        shapes
    })
}
