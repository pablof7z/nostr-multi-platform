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

use std::collections::BTreeMap;
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

/// A demand-refcounted set of [`TypedRefTarget`]s a feed session must fold
/// into its own delivery. Multiple declaring rows can demand the SAME target
/// (e.g. a comment lane and a repost lane both pointing at the same article);
/// the demand persists while at least one declarer holds it.
///
/// TODO(#3082 follow-up): demand is currently monotonic within a session's
/// lifetime — a declaring row's removal does not yet retract its demand. This
/// matches the pre-existing `pointer_target_hydration` behavior (its
/// `PointerSourceModel` is keyed by pointer event id and DOES retract on
/// pointer removal; the NEW composite-lane demand tracked here does not yet).
/// Acceptable for this PR's scope (proven by the driving-example test); a
/// full refcount-by-declaring-row-id retraction is a follow-up.
#[derive(Default)]
pub(crate) struct DeliveredRefDemand {
    // target -> number of distinct declaring rows currently demanding it.
    demand: Mutex<BTreeMap<TypedRefTarget, usize>>,
}

impl DeliveredRefDemand {
    #[must_use]
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register one more declarer for `target`.
    pub(crate) fn demand(&self, target: TypedRefTarget) {
        if let Ok(mut demand) = self.demand.lock() {
            *demand.entry(target).or_insert(0) += 1;
        }
    }

    #[must_use]
    pub(crate) fn is_demanded(&self, target: &TypedRefTarget) -> bool {
        self.demand
            .lock()
            .map(|demand| demand.contains_key(target))
            .unwrap_or(false)
    }

    #[must_use]
    pub(crate) fn targets(&self) -> Vec<TypedRefTarget> {
        self.demand
            .lock()
            .map(|demand| demand.keys().cloned().collect())
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

/// Build the admission predicate for a [`DeliveredRefDemand`]: admits an event
/// iff its kind is in `render_target_kinds` AND it is the delivered form of a
/// currently demanded target.
#[must_use]
pub(crate) fn demand_admission(
    demand: &Arc<DeliveredRefDemand>,
    render_target_kinds: Vec<u32>,
) -> RootAdmission {
    let demand = Arc::clone(demand);
    Arc::new(move |event: &KernelEvent| {
        render_target_kinds.contains(&event.kind)
            && demand.demanded_target_for_event(event).is_some()
    })
}

/// Build the live acquisition shape for a [`DeliveredRefDemand`]: the union of
/// every currently demanded target's shape.
#[must_use]
pub(crate) fn demand_live_shape(
    demand: &Arc<DeliveredRefDemand>,
    render_target_kinds: Vec<u32>,
) -> LiveShape {
    let demand = Arc::clone(demand);
    Arc::new(move || {
        let mut shape = InterestShape::default();
        let mut any = false;
        for target in demand.targets() {
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
        demand.demand(TypedRefTarget::EventId("root".to_string()));
        let admit = demand_admission(&demand, vec![30_023]);

        assert!(admit(&event("root", 30_023)));
        assert!(!admit(&event("root", 1)), "wrong kind ⇒ not admitted");
        assert!(!admit(&event("other", 30_023)), "wrong id ⇒ not admitted");
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
        demand.demand(TypedRefTarget::EventId("a".to_string()));
        demand.demand(TypedRefTarget::Address {
            kind: 30_023,
            pubkey: "bob".to_string(),
            d: "d1".to_string(),
        });
        let shape = demand_live_shape(&demand, vec![30_023])().expect("shape");
        assert!(shape.event_ids.contains("a"));
        assert_eq!(shape.addresses.len(), 1);
    }
}
