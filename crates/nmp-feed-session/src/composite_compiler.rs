//! Composite-feed compiler (#3082 settled design).
//!
//! Compiles a [`nmp_feed::CompositeFeedParams`] — an additive SET OF LANES —
//! onto the SAME `nmp_feed::FlatFeed<nmp_feed::FeedRow>` engine the degenerate
//! single-lane `FeedParams` path uses (`session_engine::flat_session`). Each
//! lane resolves its acquisition scope through the SAME step-3 compiler
//! (`resolve::resolve_scope` via `custom::resolve_acquisition`) every other
//! scope uses — there is no second acquisition resolver.
//!
//! The two engine-level additive changes this compiler exercises:
//!   1. arity-`Vec` item builder — the combined builder below runs EVERY
//!      lane whose kind/tag filter AND resolved admission claim an event, so
//!      one event can fan into multiple rows (or the delivered-target lane
//!      folds in alongside a declaring lane's placeholder row);
//!   2. the delivery-tagged `TypedRef` vector — a lane mapping's `Delivered`
//!      ref registers demand with the shared [`crate::delivered_ref`]
//!      primitive, which folds the target's key into THIS session's own
//!      admission + live shapes (never `resolve_ref`, never a store peek).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use nmp_core::substrate::{empty_suppression_lookup, KernelEvent};
use nmp_feed::{
    FeedRow, FeedSessionBuild, FlatFeedItem, FlatFeedItemBuilder, FlatFeedMerge,
    LaneMappingRegistry, MappedPayload, MappedRow, RootAdmission,
};
use nmp_planner::InterestScope;

use crate::delivered_ref::{union_admission, union_live_shape, DeliveredRefDemand};
use crate::session_engine::build_flat_scope_session;
use crate::source::{ExtraAcquisition, LiveShapes, ReducedSource};
use crate::{custom, FeedOpenError, FeedSessionHost};

/// Compile + register a composite feed session over one shared `FlatFeed`
/// engine. Returns the same [`FeedSessionBuild`] teardown-recipe contract
/// every other feed-session compile path returns.
pub fn open_composite_feed(
    app: &impl FeedSessionHost,
    params: &nmp_feed::CompositeFeedParams,
    mappings: &LaneMappingRegistry,
) -> Result<FeedSessionBuild, FeedOpenError> {
    if params.lanes.is_empty() {
        return Err(FeedOpenError::ScopeNotSupportedYet {
            scope: "CompositeFeedParams-no-lanes",
        });
    }

    let mut lanes = Vec::with_capacity(params.lanes.len());
    let mut resolved_all: Vec<ReducedSource> = Vec::with_capacity(params.lanes.len());
    for lane in &params.lanes {
        let mapping = mappings.get(&lane.mapping).ok_or_else(|| {
            revoke_all(app, &resolved_all);
            FeedOpenError::ScopeNotSupportedYet {
                scope: "CompositeFeedParams-unregistered-mapping",
            }
        })?;
        let kinds: BTreeSet<u32> = lane.match_kinds.iter().copied().collect();
        let resolved = match custom::resolve_acquisition(app, &lane.source, &kinds) {
            Ok(resolved) => resolved,
            Err(err) => {
                revoke_all(app, &resolved_all);
                return Err(err);
            }
        };
        lanes.push(CompiledLane {
            admission: resolved.admission.clone(),
            match_kinds: kinds,
            match_tags: lane
                .match_tags
                .iter()
                .map(|(key, values)| (key.0.clone(), values.clone()))
                .collect(),
            mapping,
        });
        resolved_all.push(resolved);
    }

    let render_target_kinds = params.render_target_kinds.clone();
    let demand = DeliveredRefDemand::new();

    let folded = fold_lanes(resolved_all);
    let lanes = Arc::new(lanes);

    let admission: RootAdmission = {
        let lanes = Arc::clone(&lanes);
        let lane_admission = lanes_any_admission(Arc::clone(&lanes));
        let demand_admits = union_admission(&demand, render_target_kinds.clone());
        let _ = lanes;
        Arc::new(move |event: &KernelEvent| lane_admission(event) || demand_admits(event))
    };

    let item_builder: FlatFeedItemBuilder<FeedRow> = {
        let lanes = Arc::clone(&lanes);
        let demand = Arc::clone(&demand);
        let render_target_kinds = render_target_kinds.clone();
        Arc::new(move |event: &KernelEvent| {
            build_composite_rows(&lanes, &demand, &render_target_kinds, event)
        })
    };

    let merge: FlatFeedMerge<FeedRow> = composite_merge(params.sort.clone());

    let extra_acquisition: ExtraAcquisition = {
        let base = folded.extra_acquisition.clone();
        let demand_shape = union_live_shape(&demand, render_target_kinds.clone());
        Arc::new(move || {
            let mut shapes = base();
            if let Some(shape) = demand_shape() {
                shapes.push(crate::source::AcquisitionInterest::global_with_provenance(
                    shape,
                    crate::trellis_resources::FeedSessionRouteProvenance::PointerTargetHydration,
                ));
            }
            shapes
        })
    };
    let live_shapes: LiveShapes = {
        let base = folded.live_shapes.clone();
        let demand_shape = union_live_shape(&demand, render_target_kinds);
        Arc::new(move || {
            let mut shapes = base();
            shapes.extend(demand_shape().into_iter());
            shapes
        })
    };

    let combined = ReducedSource {
        op_session_identity: folded.op_session_identity,
        admission,
        attribution: folded.attribution,
        interests: folded.interests,
        live_shape: folded.live_shape,
        live_shapes,
        observer_scope: folded.observer_scope,
        extra_acquisition,
        reactivity_hooks: folded.reactivity_hooks,
        resolver_observer_ids: folded.resolver_observer_ids,
        identity_observer_ids: folded.identity_observer_ids,
        resolver_teardown: folded.resolver_teardown,
        active_follow_set: folded.active_follow_set,
        row_context: folded.row_context,
    };

    // Retraction wiring (#3087): fires the instant `FlatFeed` drops a source
    // contribution (delete/mute/eventual eviction, via `remove_item`/
    // `remove_source`/`remove_sources_if`), releasing exactly that declaring
    // event's contribution to `demand`. Without this, `demand` only ever grew
    // for the life of the session.
    let source_removed: nmp_feed::SourceRemovedHook = {
        let demand = Arc::clone(&demand);
        Arc::new(move |source_id: &str| {
            demand.retract_source(source_id);
        })
    };

    // #3117: no composite-lane caller supplies a real suppression source yet
    // (unchanged from this path's prior, always-unsuppressed behaviour) — see
    // `session_engine`'s doc comment. Tracked as a follow-up, not silent: the
    // single-lane `FeedParams` path (`session_engine::build_scope_session_with_artifacts`)
    // threads the caller's real `Arc<dyn SuppressionLookup>`.
    build_flat_scope_session(
        app,
        params.key.as_str(),
        params.window,
        combined,
        item_builder,
        merge,
        Some(source_removed),
        empty_suppression_lookup(),
    )
}

pub(crate) struct CompiledLane {
    pub(crate) admission: RootAdmission,
    pub(crate) match_kinds: BTreeSet<u32>,
    pub(crate) match_tags: BTreeMap<String, BTreeSet<String>>,
    pub(crate) mapping: nmp_feed::LaneMapping,
}

fn lane_claims(lane: &CompiledLane, event: &KernelEvent) -> bool {
    (lane.match_kinds.is_empty() || lane.match_kinds.contains(&event.kind))
        && tags_match(&lane.match_tags, event)
        && (lane.admission)(event)
}

fn tags_match(match_tags: &BTreeMap<String, BTreeSet<String>>, event: &KernelEvent) -> bool {
    match_tags.iter().all(|(name, allowed)| {
        event.tags.iter().any(|tag| {
            tag.first().map(String::as_str) == Some(name.as_str())
                && tag.get(1).is_some_and(|value| allowed.contains(value))
        })
    })
}

fn lanes_any_admission(lanes: Arc<Vec<CompiledLane>>) -> RootAdmission {
    Arc::new(move |event: &KernelEvent| lanes.iter().any(|lane| lane_claims(lane, event)))
}

/// The combined arity-`Vec` item builder (#3082 Change 1): every lane whose
/// filter+admission claims the event contributes its mapping's rows, AND (if
/// the event is the delivered form of a currently demanded target) one
/// `FromEvent` row keyed by that target's canonical key.
pub(crate) fn build_composite_rows(
    lanes: &[CompiledLane],
    demand: &Arc<DeliveredRefDemand>,
    render_target_kinds: &[u32],
    event: &KernelEvent,
) -> Vec<FlatFeedItem<FeedRow>> {
    let mut rows = Vec::new();
    for lane in lanes {
        if !lane_claims(lane, event) {
            continue;
        }
        for mapped in (lane.mapping)(event) {
            for typed_ref in &mapped.refs {
                if typed_ref.delivery_mode == nmp_feed::DeliveryMode::Delivered {
                    // Keyed by the DECLARING EVENT's own id (#3087), i.e. the
                    // `FlatFeedItem::source_id` `build_row` below hands
                    // `FlatFeed` — NOT `mapped.canonical_row_id`. A lane
                    // mapping (e.g. `nip22_root_mapping`) can key its row by
                    // the TARGET's own coordinate (merging the declaring
                    // comment/repost onto the same row as the article it
                    // points at), so the row id is not a stable proxy for
                    // "this one declaring event" — only `event.id` is.
                    // `retract_source` (wired as the engine's
                    // `SourceRemovedHook`) fires with exactly this key once
                    // this event's own source contribution is removed.
                    demand.demand(&event.id, typed_ref.target.clone());
                }
            }
            rows.push(build_row(event, mapped));
        }
    }
    if render_target_kinds.contains(&event.kind) {
        if let Some(target) = demand.demanded_target_for_event(event) {
            rows.push(build_row(
                event,
                MappedRow {
                    canonical_row_id: target.canonical_key(),
                    payload: MappedPayload::FromEvent,
                    context: vec![nmp_feed::FeedRowContext::Authored],
                    refs: Vec::new(),
                },
            ));
        }
    }
    rows
}

fn build_row(event: &KernelEvent, mapped: MappedRow) -> FlatFeedItem<FeedRow> {
    let row = match mapped.payload {
        MappedPayload::FromEvent => FeedRow {
            canonical_row_id: mapped.canonical_row_id.clone(),
            source_id: event.id.clone(),
            author_pubkey: event.author.clone(),
            kind: event.kind,
            created_at: event.created_at,
            content: event.content.clone(),
            tags: event.tags.clone(),
            relay_provenance: event.received_from_relays(),
            refs: mapped.refs,
            context: mapped.context,
        },
        MappedPayload::Placeholder => FeedRow {
            canonical_row_id: mapped.canonical_row_id.clone(),
            source_id: event.id.clone(),
            author_pubkey: String::new(),
            kind: 0,
            created_at: 0,
            content: String::new(),
            tags: Vec::new(),
            relay_provenance: Vec::new(),
            refs: mapped.refs,
            context: mapped.context,
        },
        MappedPayload::Explicit(fields) => FeedRow {
            canonical_row_id: mapped.canonical_row_id.clone(),
            source_id: event.id.clone(),
            author_pubkey: fields.author_pubkey,
            kind: fields.kind,
            created_at: fields.created_at,
            content: fields.content,
            tags: fields.tags,
            relay_provenance: event.received_from_relays(),
            refs: mapped.refs,
            context: mapped.context,
        },
    };
    FlatFeedItem {
        id: mapped.canonical_row_id,
        source_id: event.id.clone(),
        sort_created_at: event.created_at,
        card: row,
    }
}

/// Compile a [`nmp_feed::SortPolicy`] to the engine's [`FlatFeedMerge`].
///
/// Both policies ACCUMULATE the provenance-context SET and the ref vector
/// across every source contributing to a canonical row (`merge_context` /
/// `merge_refs`) — the difference is only which source's PAYLOAD (and sort
/// position) wins.
pub(crate) fn composite_merge(sort: nmp_feed::SortPolicy) -> FlatFeedMerge<FeedRow> {
    match sort {
        nmp_feed::SortPolicy::ByInteractionTime => Arc::new(|existing, incoming| {
            let Some(existing) = existing else {
                return incoming;
            };
            // `merge_sources` folds contributions in DESCENDING
            // `sort_created_at` order, so `existing` is always the
            // highest-sorted contribution seen so far — its
            // `sort_created_at`/id/source_id give the row's newest-interaction
            // SORT POSITION (e.g. a repost bumping its target to the top).
            // The CARD payload, though, must prefer whichever side is
            // actually hydrated: a placeholder (no embedded/delivered content
            // yet) must never shadow an already-hydrated contribution just
            // because it happens to sort later (e.g. a bare repost bumping an
            // already-admitted target note) — the same "prefer hydrated"
            // outcome the pre-composite follows-timeline merge gave (#3092).
            let context = nmp_feed::merge_context(&existing.card.context, &incoming.card.context);
            let refs = nmp_feed::merge_refs(&existing.card.refs, &incoming.card.refs);
            let mut card = if existing.card.is_placeholder() && !incoming.card.is_placeholder() {
                incoming.card.clone()
            } else {
                existing.card.clone()
            };
            card.context = context;
            card.refs = refs;
            FlatFeedItem {
                card,
                ..existing.clone()
            }
        }),
        nmp_feed::SortPolicy::ByTargetCreatedAt => Arc::new(|existing, incoming| {
            let Some(existing) = existing else {
                return incoming;
            };
            let existing_hydrated = !existing.card.is_placeholder();
            let incoming_hydrated = !incoming.card.is_placeholder();
            let context = nmp_feed::merge_context(&existing.card.context, &incoming.card.context);
            let refs = nmp_feed::merge_refs(&existing.card.refs, &incoming.card.refs);
            match (existing_hydrated, incoming_hydrated) {
                // Both sides are real, hydrated revisions of the SAME
                // canonical coordinate (e.g. two delivered revisions of a
                // replaceable kind:30023 target). `merge_sources` folds
                // sources in `sort_created_at`-DESCENDING order and
                // terminates on the lowest, so `existing` is typically the
                // newer accumulator and `incoming` the older fold step —
                // unconditionally adopting `incoming` here silently regressed
                // to the OLDER revision. Adopt whichever side carries the
                // newer `created_at` (newest revision wins), never a fixed
                // side.
                (true, true) => {
                    let newer = if incoming.sort_created_at >= existing.sort_created_at {
                        incoming.clone()
                    } else {
                        existing.clone()
                    };
                    let mut card = newer.card.clone();
                    card.context = context;
                    card.refs = refs;
                    FlatFeedItem {
                        id: newer.id.clone(),
                        source_id: newer.source_id.clone(),
                        sort_created_at: newer.sort_created_at,
                        card,
                    }
                }
                // Only the incoming delivery is hydrated: adopt its payload
                // and its true `created_at` as the row's sort key.
                (false, true) => {
                    let mut card = incoming.card.clone();
                    card.context = context;
                    card.refs = refs;
                    FlatFeedItem {
                        id: incoming.id.clone(),
                        source_id: incoming.source_id.clone(),
                        sort_created_at: incoming.sort_created_at,
                        card,
                    }
                }
                // Existing is already hydrated; incoming is a placeholder —
                // keep the hydrated payload, just accumulate provenance.
                (true, false) => {
                    let mut card = existing.card.clone();
                    card.context = context;
                    card.refs = refs;
                    FlatFeedItem {
                        card,
                        ..existing.clone()
                    }
                }
                // Neither hydrated yet: keep the newest placeholder as the
                // provisional interaction-time proxy, accumulating provenance.
                (false, false) => {
                    let newer = if existing.sort_created_at >= incoming.sort_created_at {
                        existing.clone()
                    } else {
                        incoming.clone()
                    };
                    let mut card = newer.card.clone();
                    card.context = context;
                    card.refs = refs;
                    FlatFeedItem { card, ..newer }
                }
            }
        }),
    }
}

/// Fold every lane's resolved acquisition into ONE combined [`ReducedSource`]
/// for the cross-cutting fields `build_flat_scope_session` consumes
/// (`interests`/`live_shapes`/`observer_scope`/`extra_acquisition`/
/// `reactivity_hooks`/teardown ids). `admission`/`attribution`/`live_shape`/
/// `row_context` are per-lane concerns the compiler handles separately (see
/// `build_composite_rows`); the folded copies of those fields are discarded by
/// `build_flat_scope_session` (unused there for the single-lane path too).
fn fold_lanes(mut sources: Vec<ReducedSource>) -> ReducedSource {
    let mut acc = sources.remove(0);
    for next in sources {
        acc.op_session_identity = acc.op_session_identity.combine(next.op_session_identity);
        acc.interests.extend(next.interests);
        acc.observer_scope = combine_observer_scope(acc.observer_scope, next.observer_scope);
        let left_live_shapes = acc.live_shapes.clone();
        let right_live_shapes = next.live_shapes.clone();
        acc.live_shapes = Arc::new(move || {
            let mut shapes = left_live_shapes();
            shapes.extend(right_live_shapes());
            shapes
        });
        let left_extra = acc.extra_acquisition.clone();
        let right_extra = next.extra_acquisition.clone();
        acc.extra_acquisition = Arc::new(move || {
            let mut shapes = left_extra();
            shapes.extend(right_extra());
            shapes
        });
        acc.reactivity_hooks.extend(next.reactivity_hooks);
        acc.resolver_observer_ids.extend(next.resolver_observer_ids);
        acc.identity_observer_ids.extend(next.identity_observer_ids);
        acc.resolver_teardown.extend(next.resolver_teardown);
        acc.active_follow_set = acc.active_follow_set.or(next.active_follow_set);
    }
    acc
}

fn combine_observer_scope(left: InterestScope, right: InterestScope) -> InterestScope {
    if matches!(left, InterestScope::Global) || matches!(right, InterestScope::Global) {
        InterestScope::Global
    } else {
        left
    }
}

/// Revoke every already-resolved lane's observers on a mid-fanout failure
/// (fail-closed, D8 — a failed open leaks nothing).
fn revoke_all(app: &impl FeedSessionHost, resolved: &[ReducedSource]) {
    for source in resolved {
        for id in &source.resolver_observer_ids {
            app.observed_projection_handle().close(*id);
        }
        for id in &source.identity_observer_ids {
            (app.unregister_identity_change_observer_action(*id))();
        }
    }
}

#[cfg(test)]
#[path = "composite_compiler_tests.rs"]
mod tests;
