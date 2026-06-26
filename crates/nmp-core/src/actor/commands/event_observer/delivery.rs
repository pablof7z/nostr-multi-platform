use super::{KernelEventObserverId, KernelEventObserverSlot};
use crate::planner::InterestShape;
use crate::substrate::{observed_shape_matches_event, KernelEvent};

/// Live-delivery mode for one Rust-trait observer registration.
pub(super) enum RustObserverDelivery {
    /// Registered but muted: skipped by global `notify_observers` and reached
    /// only by `notify_observer_by_id` during targeted replay.
    Muted,
    /// Legacy live tap: receives every accepted event.
    ActiveAll,
    /// Observed projection: receives only accepted events matching at least one
    /// declared interest shape.
    Scoped(Vec<InterestShape>),
}

impl RustObserverDelivery {
    pub(super) fn matches(&self, event: &KernelEvent) -> bool {
        match self {
            Self::Muted => false,
            Self::ActiveAll => true,
            Self::Scoped(shapes) => shapes
                .iter()
                .any(|shape| observed_shape_matches_event(shape, event)),
        }
    }
}

/// Activate a previously muted observer (ADR-0062).
///
/// Sets the registration matching `id` to unfiltered live-tap delivery, so
/// subsequent `notify_observers` calls include it for every accepted event.
/// Returns `true` iff a registration was found; returns `false` for unknown ids
/// (idempotent — safe to call even if the registration was removed before
/// activation, e.g. the screen was closed before the replay command was
/// dispatched).
pub fn activate_observer(slot: &KernelEventObserverSlot, id: KernelEventObserverId) -> bool {
    let Ok(mut guard) = slot.lock() else {
        return false;
    };
    for reg in &mut guard.rust {
        if reg.id == id {
            reg.delivery = RustObserverDelivery::ActiveAll;
            return true;
        }
    }
    false
}

/// Activate a previously muted observer for a declared observed projection.
///
/// Unlike [`activate_observer`], this does NOT join the unfiltered global
/// fan-out. Subsequent live delivery is constrained to `shape`, while targeted
/// replay remains able to deliver directly by id.
pub fn activate_observer_scoped(
    slot: &KernelEventObserverSlot,
    id: KernelEventObserverId,
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
                RustObserverDelivery::ActiveAll => {}
            }
            return true;
        }
    }
    false
}
