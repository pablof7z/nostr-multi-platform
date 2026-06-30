//! Map the read model's demanded [`EmbedTarget`] set onto kernel interest
//! shapes: per-target dependent children for acquisition, and one union shape
//! for delivery.

use std::collections::BTreeSet;

use nmp_core::DependentInterestChild;
use nmp_planner::{InterestScope, InterestShape, NaddrCoord};

use crate::EmbedTarget;

use super::PointerSourceModel;

/// The interest shape that hydrates a single target: an `event_ids` predicate
/// for an event-id target, an `addresses` predicate for an address target.
fn target_shape(target: &EmbedTarget) -> InterestShape {
    match target {
        EmbedTarget::Event(id) => InterestShape {
            event_ids: BTreeSet::from([id.clone()]),
            ..InterestShape::default()
        },
        EmbedTarget::Address {
            kind,
            pubkey,
            identifier,
        } => InterestShape {
            // The addressable kind is carried alongside the coordinate: the
            // store's cache-serve plan rejects a wildcard-kinds shape before it
            // reaches the address (`KindDtag`) arm, so an address target hydrates
            // from cache only when its kind travels with it.
            kinds: BTreeSet::from([*kind]),
            addresses: BTreeSet::from([NaddrCoord {
                pubkey: pubkey.clone(),
                kind: *kind,
                d_tag: identifier.clone(),
            }]),
            ..InterestShape::default()
        },
    }
}

fn interest_scope(scope: u32) -> InterestScope {
    if scope == 0 {
        InterestScope::ActiveAccount
    } else {
        InterestScope::Global
    }
}

/// One dependent child per demanded target. Per-target children (rather than one
/// batched child) give the kernel registry true cross-consumer dedup: two read
/// models that both point at the same target emit the identical `tailing` child,
/// which shares a `SubKey` and collapses onto one live registry slot.
pub(crate) fn target_children(
    model: &PointerSourceModel,
    scope: u32,
) -> Vec<DependentInterestChild> {
    let scope = interest_scope(scope);
    model
        .target_demand()
        .map(|target| DependentInterestChild::tailing(target_shape(target), scope.clone()))
        .collect()
}

/// The union delivery shape covering every demanded target (`event_ids` ∪
/// `addresses`). Returns `None` when nothing is demanded, so an empty reduction
/// opens no delivery interest (fail closed — never a wildcard query).
pub(crate) fn delivery_shape(model: &PointerSourceModel) -> Option<InterestShape> {
    let mut shape = InterestShape::default();
    for target in model.target_demand() {
        match target {
            EmbedTarget::Event(id) => {
                shape.event_ids.insert(id.clone());
            }
            EmbedTarget::Address {
                kind,
                pubkey,
                identifier,
            } => {
                shape.kinds.insert(*kind);
                shape.addresses.insert(NaddrCoord {
                    pubkey: pubkey.clone(),
                    kind: *kind,
                    d_tag: identifier.clone(),
                });
            }
        }
    }
    if shape.event_ids.is_empty() && shape.addresses.is_empty() {
        None
    } else {
        Some(shape)
    }
}
