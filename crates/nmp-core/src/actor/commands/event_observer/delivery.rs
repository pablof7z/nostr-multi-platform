use super::{ObservedProjectionId, ObservedProjectionSinkSlot};
use crate::planner::InterestShape;
use crate::substrate::{observed_shape_matches_event, KernelEvent};

/// Live-delivery mode for one sink registration.
pub(super) enum RustObserverDelivery {
    /// Registered but muted: skipped by global `notify_observers` and reached
    /// only by `notify_observer_by_id` during targeted replay.
    Muted,
    /// Observed projection: receives only accepted events matching at least one
    /// declared interest shape.
    Scoped(Vec<InterestShape>),
}

impl RustObserverDelivery {
    #[cfg(test)]
    pub(super) fn test_all_events() -> Self {
        Self::Scoped(vec![InterestShape::default()])
    }

    pub(super) fn matches(&self, event: &KernelEvent) -> bool {
        match self {
            Self::Muted => false,
            Self::Scoped(shapes) => shapes
                .iter()
                .any(|shape| observed_shape_matches_event(shape, event)),
        }
    }
}

/// Activate a previously muted observer for a declared observed projection.
///
/// This does not join an unfiltered global fan-out. Subsequent live delivery is
/// constrained to `shape`, while targeted replay remains able to deliver
/// directly by id.
pub fn activate_observer_scoped(
    slot: &ObservedProjectionSinkSlot,
    id: ObservedProjectionId,
    shape: InterestShape,
) -> bool {
    let Ok(mut guard) = slot.lock() else {
        return false;
    };
    for reg in &mut guard.rust {
        if reg.id == id {
            match &mut reg.delivery {
                RustObserverDelivery::Muted => {
                    reg.delivery = RustObserverDelivery::Scoped(vec![shape]);
                }
                RustObserverDelivery::Scoped(shapes) => {
                    if !shapes.contains(&shape) {
                        shapes.push(shape);
                    }
                }
            }
            return true;
        }
    }
    false
}
