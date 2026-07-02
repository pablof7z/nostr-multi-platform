//! Off-lock event pre-translation for the [`super::Pool`] translator.
//!
//! Split out of `pool::inner` (file-size ownership). The expensive part of
//! turning a `RelayEvent` into a `PoolEvent` — JSON-parsing every text frame to
//! pre-classify NIP-42 AUTH — happens in [`prepare_event`], which runs BEFORE
//! the translator takes the `PoolInner` lock. [`apply_prepared`] then runs the
//! O(1) remainder (generation check + health mutation + event build) under the
//! lock, so a per-frame parse never blocks concurrent `Pool::send` calls.

use crate::relay_worker::RelayEvent;

use super::frame::tungstenite_to_relay_frame;
use super::inner::{PoolInner, SlotState};
use super::types::{ClosedReason, HealthState, PoolEvent, RelayFrame, RelayHandle, TransportError};

/// A `RelayEvent` with the lock-free portion of translation already done.
/// Carries the routing key `(relay_url, generation)` plus the variant-specific
/// payload that the (locked) `apply_prepared` step turns into a `PoolEvent`.
pub(super) enum PreparedEvent {
    Connected {
        relay_url: String,
        generation: u64,
    },
    Failed {
        relay_url: String,
        generation: u64,
        error: String,
        permanent: bool,
    },
    Closed {
        relay_url: String,
        generation: u64,
    },
    /// The `tungstenite::Message → RelayFrame` conversion (incl. the AUTH
    /// JSON pre-classification) has already happened lock-free.
    Frame {
        relay_url: String,
        generation: u64,
        frame: RelayFrame,
    },
}

impl PreparedEvent {
    fn relay_url(&self) -> &str {
        match self {
            Self::Connected { relay_url, .. }
            | Self::Failed { relay_url, .. }
            | Self::Closed { relay_url, .. }
            | Self::Frame { relay_url, .. } => relay_url,
        }
    }

    fn generation(&self) -> u64 {
        match self {
            Self::Connected { generation, .. }
            | Self::Failed { generation, .. }
            | Self::Closed { generation, .. }
            | Self::Frame { generation, .. } => *generation,
        }
    }
}

/// Lock-free pre-translation: classify the error, and (for `Message`) run the
/// `tungstenite::Message → RelayFrame` conversion including the AUTH JSON
/// pre-classification. Returns `None` only for a raw `Message::Frame(_)` (the
/// kernel never observes it) — nothing to deliver.
pub(super) fn prepare_event(event: RelayEvent) -> Option<PreparedEvent> {
    Some(match event {
        RelayEvent::Connected {
            relay_url,
            generation,
        } => PreparedEvent::Connected {
            relay_url,
            generation,
        },
        RelayEvent::Failed {
            relay_url,
            generation,
            error,
        } => {
            let permanent = crate::relay_protocol::is_permanent_error(&error);
            PreparedEvent::Failed {
                relay_url,
                generation,
                error,
                permanent,
            }
        }
        RelayEvent::Closed {
            relay_url,
            generation,
        } => PreparedEvent::Closed {
            relay_url,
            generation,
        },
        RelayEvent::Message {
            relay_url,
            generation,
            message,
        } => {
            // The expensive bit — JSON-parse-on-text — happens here, off-lock.
            let frame = tungstenite_to_relay_frame(message)?;
            PreparedEvent::Frame {
                relay_url,
                generation,
                frame,
            }
        }
    })
}

/// Apply a [`PreparedEvent`] to the slot and build the outbound [`PoolEvent`].
/// Runs under the `PoolInner` lock; O(1) — no JSON, no allocation beyond the
/// returned event. Returns `None` if the event is stale (slot reopened/closed).
pub(super) fn apply_prepared(inner: &mut PoolInner, event: PreparedEvent) -> Option<PoolEvent> {
    let (slot_id, state) = locate_slot(inner, event.relay_url(), event.generation())?;
    let h = RelayHandle {
        slot: slot_id,
        generation: state.generation,
    };
    Some(match event {
        PreparedEvent::Connected {
            relay_url,
            generation,
        } => {
            state.health.state = HealthState::Connected;
            state.health.connect_count = state.health.connect_count.saturating_add(1);
            state.health.last_error = None;
            PoolEvent::Opened {
                h,
                url: relay_url,
                generation,
            }
        }
        PreparedEvent::Failed {
            relay_url: _,
            generation,
            error,
            permanent,
        } => {
            state.health.failure_count = state.health.failure_count.saturating_add(1);
            state.health.last_error = Some(error.clone());
            if matches!(state.health.state, HealthState::Connected) {
                state.health.state = HealthState::Reconnecting;
            }
            if permanent {
                state.health.state = HealthState::Closed;
            }
            PoolEvent::Failed {
                h,
                generation,
                error: TransportError {
                    message: error,
                    permanent,
                },
            }
        }
        PreparedEvent::Closed {
            relay_url: _,
            generation,
        } => {
            // The worker emits `Closed` only after acking a Shutdown
            // command (the normal-close path). Slot-level closes via
            // `Pool::close` surface as `ClosedReason::Requested`; the
            // `Pool::shutdown` bulk-close path short-circuits at the
            // top of `translator_loop` (no per-slot event), so we
            // never need to distinguish here.
            state.health.state = HealthState::Closed;
            PoolEvent::Closed {
                h,
                generation,
                reason: ClosedReason::Requested,
            }
        }
        PreparedEvent::Frame {
            relay_url: _,
            generation,
            frame,
        } => PoolEvent::Frame {
            h,
            generation,
            frame,
        },
    })
}

/// Find the slot for `(url, generation)` and return a mutable
/// reference. Returns `None` if the slot has since been reused (the
/// stored generation is higher) or the URL was never inserted.
fn locate_slot<'a>(
    inner: &'a mut PoolInner,
    url: &str,
    generation: u64,
) -> Option<(u32, &'a mut SlotState)> {
    // The worker emits the URL it dialled (already canonicalized by
    // `ensure_open`), so the inner map's key matches byte-for-byte.
    let slot_id = *inner.url_to_slot.get(url)?;
    let state = inner.slots.get_mut(slot_id as usize)?.as_mut()?;
    if state.generation != generation {
        // Stale event — the slot has been reopened (or closed) since
        // the worker emitted this event. Drop.
        return None;
    }
    Some((slot_id, state))
}
