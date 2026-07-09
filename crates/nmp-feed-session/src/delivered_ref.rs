//! Shared `Delivered`-ref demand → admission/shape mechanism (#3082 settled
//! design).
//!
//! A [`nmp_feed::TypedRef`] with [`nmp_feed::DeliveryMode::Delivered`] asks the
//! OWNING feed session to fold its target's key into the session's own
//! `live_shapes` + admission, so the target re-enters `on_kernel_event` as a
//! real delivered event (true `created_at`, its own provenance contribution).
//!
//! This is the SAME mechanism `pointer_target_hydration.rs` pioneered for
//! reaction/comment pointer-target hydration (`FeedScope::PointerTargetHydration`).
//! That module used to carry its own private `target_shape` /
//! `target_is_demanded` / `target_delivery_shape` helpers over
//! `nmp_content::EmbedTarget`; they are GENERALIZED here over
//! [`nmp_feed::TypedRefTarget`] so the composite-lane compiler
//! (`composite_compiler.rs`) can share the identical admission/shape logic
//! instead of re-deriving a second, disjoint implementation of "demand a
//! target, admit it, expand acquisition" (#3085's sibling concern: one
//! mechanism, not two).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_feed::{RootAdmission, TypedRefTarget};
use nmp_nip09::AddressCoordinate;
use nmp_planner::{InterestShape, NaddrCoord};

use crate::source::LiveShape;

/// The acquisition shape for one demanded [`TypedRefTarget`], restricted to
/// `render_target_kinds` (the delivered target's own kind, distinct from
/// whatever kind acquired the DECLARING pointer/wrapper event).
#[must_use]
pub(crate) fn typed_ref_target_shape(
    target: &TypedRefTarget,
    render_target_kinds: &[u32],
) -> Option<InterestShape> {
    match target {
        TypedRefTarget::EventId(id) => Some(InterestShape {
            event_ids: std::collections::BTreeSet::from([id.clone()]),
            kinds: render_target_kinds.iter().copied().collect(),
            ..InterestShape::default()
        }),
        TypedRefTarget::Address { kind, pubkey, d } => {
            render_target_kinds.contains(kind).then(|| InterestShape {
                kinds: std::collections::BTreeSet::from([*kind]),
                addresses: std::collections::BTreeSet::from([NaddrCoord {
                    pubkey: pubkey.clone(),
                    kind: *kind,
                    d_tag: d.clone(),
                }]),
                ..InterestShape::default()
            })
        }
    }
}

/// Whether `event` is the delivered form of `target`.
#[must_use]
pub(crate) fn typed_ref_target_matches(target: &TypedRefTarget, event: &KernelEvent) -> bool {
    match target {
        TypedRefTarget::EventId(id) => &event.id == id,
        TypedRefTarget::Address { kind, pubkey, d } => AddressCoordinate::from_event(event)
            .is_some_and(|coord| {
                coord.kind == *kind && &coord.pubkey == pubkey && &coord.identifier == d
            }),
    }
}

/// Reverse-indexed demand state, mirroring [`nmp_content::PointerSourceModel`]'s
/// `pointers` / `pointed_by` pair (#3087): `by_source` lets a declaring
/// event's removal find exactly the targets it contributed without a scan,
/// and `demanded_by` is the live materialization demand — a target with an
/// empty declarer set is removed entirely, so its key set IS the demand.
#[derive(Default)]
struct DeliveredRefDemandState {
    /// Declaring event's own id (`FlatFeedItem::source_id`) -> targets that
    /// event currently demands.
    by_source: BTreeMap<String, BTreeSet<TypedRefTarget>>,
    /// Target -> declaring event ids currently demanding it.
    demanded_by: BTreeMap<TypedRefTarget, BTreeSet<String>>,
}

/// A demand-refcounted set of [`TypedRefTarget`]s a feed session must fold
/// into its own delivery. Multiple declaring events can demand the SAME
/// target (e.g. a comment and a repost both pointing at the same article);
/// the demand persists while at least one declaring event's source
/// contribution is still live, and RETRACTS the instant the last one is
/// removed (#3087) — the same contract
/// [`nmp_content::PointerSourceModel::drop_pointer`] gives pointer targets,
/// keyed here by the declaring event's own id (its
/// [`nmp_feed::FlatFeedItem::source_id`]) rather than a canonical row id: a
/// composite row's id can equal one of its OWN contributing sources' target
/// (`nip22_root_mapping` keys a comment's row by the article it points at),
/// so the row id is not a stable proxy for "this one declaring event" — only
/// the event's own id is. Before #3087 this was monotonic — `demand()` only
/// ever incremented a bare counter, so a declaring event's removal
/// (delete/mute/eviction) never released its target's subscription, growing
/// acquisition unboundedly over a long session.
#[derive(Default)]
pub(crate) struct DeliveredRefDemand {
    state: Mutex<DeliveredRefDemandState>,
}

impl DeliveredRefDemand {
    #[must_use]
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register `source_id` (a declaring event's own id) as one more declarer
    /// of `target`. Idempotent: re-registering the same `(source_id, target)`
    /// pair (e.g. a re-delivered wrapper event on reconnect) does not
    /// double-count, so a single [`Self::retract_source`] fully withdraws it
    /// regardless of how many times `demand` ran.
    pub(crate) fn demand(&self, source_id: &str, target: TypedRefTarget) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state
            .by_source
            .entry(source_id.to_string())
            .or_default()
            .insert(target.clone());
        state
            .demanded_by
            .entry(target)
            .or_default()
            .insert(source_id.to_string());
    }

    /// Retract every target `source_id` declared demand for — that declaring
    /// event's own source contribution was removed (deleted, muted, or
    /// otherwise dropped from the feed). A target's demand is withdrawn only
    /// once NO declaring event still names it (another event may still hold
    /// the same target). Returns whether the demanded target SET shrank (a
    /// target's subscription was actually released), mirroring
    /// [`nmp_content::PointerSourceModel::drop_pointer`]'s return contract.
    ///
    /// Named to match [`nmp_feed::SourceRemovedHook`]'s call site in
    /// `composite_compiler.rs` (`retract_source` there wires this as the
    /// engine's per-source removal hook) — the parameter is a source id, not
    /// a row id; see the struct docs above for why those differ here.
    pub(crate) fn retract_source(&self, source_id: &str) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(targets) = state.by_source.remove(source_id) else {
            return false;
        };
        let mut shrank = false;
        for target in targets {
            if let Some(declarers) = state.demanded_by.get_mut(&target) {
                declarers.remove(source_id);
                if declarers.is_empty() {
                    state.demanded_by.remove(&target);
                    shrank = true;
                }
            }
        }
        shrank
    }

    #[must_use]
    pub(crate) fn targets(&self) -> Vec<TypedRefTarget> {
        self.state
            .lock()
            .map(|state| state.demanded_by.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether `event` is the delivered form of any currently demanded target,
    /// returning that target's canonical key when so.
    #[must_use]
    pub(crate) fn demanded_target_for_event(&self, event: &KernelEvent) -> Option<TypedRefTarget> {
        self.targets()
            .into_iter()
            .find(|target| typed_ref_target_matches(target, event))
    }
}

/// Anything that can report its CURRENT set of demanded [`TypedRefTarget`]s.
///
/// Implemented once by both demand sources this crate has — [`DeliveredRefDemand`]
/// (composite-lane `Delivered` refs, refcounted by declaring event id and
/// retracted via [`DeliveredRefDemand::retract_source`] once no declaring event
/// still names a target, #3087) and [`nmp_content::PointerSourceModel`]
/// (`pointer_target_hydration`'s pointer model, which retracts a target once
/// no live pointer still names it) — so [`union_admission`]/[`union_live_shape`]
/// are the ONE union builder over "whatever is currently demanded", not two
/// disjoint copies of the same union loop (#3082 SHOULD-FIX 5, the sibling of
/// #3085's `resolve_ref` unification: the demand SOURCE differs, the union
/// math over it must not).
pub(crate) trait DemandedTargets {
    fn demanded_targets(&self) -> Vec<TypedRefTarget>;
}

impl DemandedTargets for DeliveredRefDemand {
    fn demanded_targets(&self) -> Vec<TypedRefTarget> {
        self.targets()
    }
}

/// Build the admission predicate over any [`DemandedTargets`] source: admits
/// an event iff its kind is in `render_target_kinds` AND it is the delivered
/// form of a currently demanded target.
#[must_use]
pub(crate) fn union_admission<D>(demand: &Arc<D>, render_target_kinds: Vec<u32>) -> RootAdmission
where
    D: DemandedTargets + Send + Sync + 'static,
{
    let demand = Arc::clone(demand);
    Arc::new(move |event: &KernelEvent| {
        render_target_kinds.contains(&event.kind)
            && demand
                .demanded_targets()
                .iter()
                .any(|target| typed_ref_target_matches(target, event))
    })
}

/// Build the live acquisition shape over any [`DemandedTargets`] source: the
/// union of every currently demanded target's shape.
#[must_use]
pub(crate) fn union_live_shape<D>(demand: &Arc<D>, render_target_kinds: Vec<u32>) -> LiveShape
where
    D: DemandedTargets + Send + Sync + 'static,
{
    let demand = Arc::clone(demand);
    Arc::new(move || {
        let mut shape = InterestShape::default();
        let mut any = false;
        for target in demand.demanded_targets() {
            if let Some(target_shape) = typed_ref_target_shape(&target, &render_target_kinds) {
                shape.kinds.extend(target_shape.kinds);
                shape.event_ids.extend(target_shape.event_ids);
                shape.addresses.extend(target_shape.addresses);
                any = true;
            }
        }
        any.then_some(shape)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::EventId;

    fn event(id: &str, kind: u32) -> KernelEvent {
        KernelEvent {
            id: EventId::from(id),
            author: "author".to_string(),
            kind,
            created_at: 100,
            tags: Vec::new(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn event_id_target_admits_only_the_matching_id_and_kind() {
        let demand = DeliveredRefDemand::new();
        demand.demand("comment-1", TypedRefTarget::EventId("root".to_string()));
        let admit = union_admission(&demand, vec![30_023]);

        assert!(admit(&event("root", 30_023)));
        assert!(!admit(&event("root", 1)), "wrong kind ⇒ not admitted");
        assert!(!admit(&event("other", 30_023)), "wrong id ⇒ not admitted");
    }

    /// #3087 regression: before the refcounted keying, `demand()` only ever
    /// incremented a bare counter and nothing could ever retract it — a
    /// declaring event's removal left its target's subscription live
    /// forever. This proves the OLD shape of the bug is gone: retracting the
    /// ONLY declaring event withdraws the target from both admission and
    /// live shape.
    #[test]
    fn retracting_the_only_declaring_event_withdraws_the_target() {
        let demand = DeliveredRefDemand::new();
        demand.demand("comment-1", TypedRefTarget::EventId("root".to_string()));
        assert_eq!(
            demand.targets().len(),
            1,
            "target demanded while declaring event lives"
        );

        let shrank = demand.retract_source("comment-1");
        assert!(shrank, "removing the last declarer must shrink demand");
        assert!(
            demand.targets().is_empty(),
            "target must be undemanded once its only declaring event is removed"
        );

        let admit = union_admission(&demand, vec![30_023]);
        assert!(
            !admit(&event("root", 30_023)),
            "a retracted target must no longer be admitted (subscription released)"
        );
        assert!(
            union_live_shape(&demand, vec![30_023])().is_none(),
            "a retracted target must not appear in the live acquisition shape"
        );
    }

    /// Two declaring events (e.g. a comment and a repost) can demand the SAME
    /// target; removing one must not drop the other's live demand — only the
    /// last declarer's removal releases the subscription.
    #[test]
    fn target_demanded_by_two_events_survives_removal_of_one() {
        let demand = DeliveredRefDemand::new();
        let target = TypedRefTarget::EventId("shared-target".to_string());
        demand.demand("comment-1", target.clone());
        demand.demand("repost-1", target.clone());

        let shrank = demand.retract_source("comment-1");
        assert!(
            !shrank,
            "another declaring event still holds the target ⇒ demand must not shrink"
        );
        assert_eq!(
            demand.targets(),
            vec![target.clone()],
            "target stays demanded while repost-1 still declares it"
        );

        let shrank = demand.retract_source("repost-1");
        assert!(shrank, "the LAST declarer's removal must retract the target");
        assert!(demand.targets().is_empty());
    }

    /// Re-registering the same (event, target) pair (a re-delivered wrapper
    /// event on reconnect) must not require a matching number of
    /// retractions — one `retract_source` fully withdraws that event's demand
    /// regardless of how many times `demand()` ran for it.
    #[test]
    fn re_demanding_the_same_event_and_target_is_idempotent_for_retraction() {
        let demand = DeliveredRefDemand::new();
        let target = TypedRefTarget::EventId("root".to_string());
        demand.demand("comment-1", target.clone());
        demand.demand("comment-1", target.clone());
        demand.demand("comment-1", target);

        assert!(demand.retract_source("comment-1"));
        assert!(demand.targets().is_empty());
        assert!(
            !demand.retract_source("comment-1"),
            "retracting an already-retracted event is a no-op, not a re-shrink"
        );
    }

    /// Retracting an event that never declared any demand must be a harmless
    /// no-op — e.g. the delivered target's own event, or a lane row with no
    /// `Delivered` refs, both of which are legitimately removed without ever
    /// having called `demand()`.
    #[test]
    fn retracting_an_undemanding_event_is_a_no_op() {
        let demand = DeliveredRefDemand::new();
        demand.demand("comment-1", TypedRefTarget::EventId("root".to_string()));

        assert!(!demand.retract_source("never-demanded-event"));
        assert_eq!(
            demand.targets().len(),
            1,
            "unrelated retraction must not touch other demand"
        );
    }

    #[test]
    fn address_target_shape_is_gated_by_render_target_kinds() {
        let target = TypedRefTarget::Address {
            kind: 30_023,
            pubkey: "author".to_string(),
            d: "article".to_string(),
        };
        assert!(typed_ref_target_shape(&target, &[30_023]).is_some());
        assert!(
            typed_ref_target_shape(&target, &[1]).is_none(),
            "kind not in render_target_kinds ⇒ no shape"
        );
    }

    #[test]
    fn live_shape_unions_every_demanded_target() {
        let demand = DeliveredRefDemand::new();
        demand.demand("comment-1", TypedRefTarget::EventId("a".to_string()));
        demand.demand(
            "repost-1",
            TypedRefTarget::Address {
                kind: 30_023,
                pubkey: "bob".to_string(),
                d: "d1".to_string(),
            },
        );
        let shape = union_live_shape(&demand, vec![30_023])().expect("shape");
        assert!(shape.event_ids.contains("a"));
        assert_eq!(shape.addresses.len(), 1);
    }
}
