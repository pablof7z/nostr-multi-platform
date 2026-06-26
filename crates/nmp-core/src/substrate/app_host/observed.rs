//! Live-tap and observed-projection registration seams.
//!
//! Split out of `app_host/mod.rs` (file-size ceiling, AGENTS.md) — these two
//! traits + the `ObservedProjection` declaration bundle are one cohesive
//! concern: how a host installs a `KernelEventObserver`, either as a bare live
//! tap (no replay) or as a replay-safe observed projection.

use std::sync::Arc;

use crate::{KernelEventObserver, KernelEventObserverId};

/// Register / unregister kernel-event observers — the **live-tap** seam.
///
/// `register_live_event_tap` registers an ACTIVE observer with NO replay, so a
/// hydrated read-model registered after its interest warmed the cache silently
/// misses already-cached events.  Callers that need replay must use
/// [`ObservedProjectionRegistrar::open_observed_projection`] instead, which
/// pairs the observer with a muted→activate replay sequence.
///
/// This trait (and its rename from `EventObserverRegistrar`) exists to surface
/// the footgun at the call site: `register_live_event_tap` says exactly what it
/// does — a tap on the live ingest stream, no past events.
pub trait LiveEventTapRegistrar {
    fn register_live_event_tap(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId;

    fn unregister_event_observer(&self, id: KernelEventObserverId);

    fn swap_singleton_event_observer(
        &self,
        new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId>;
}

/// Register and close **observed projections** — the safe alternative to the
/// live-tap seam when replay of already-cached events is required.
///
/// [`open_observed_projection`](ObservedProjectionRegistrar::open_observed_projection)
/// combines observer registration (muted), an interest open, and a
/// kernel-side muted→activate replay sequence in a single call, so the
/// observer cannot miss events that arrived before it was registered.
/// [`close_observed_projection`](ObservedProjectionRegistrar::close_observed_projection)
/// reverses both registrations atomically.
pub trait ObservedProjectionRegistrar {
    fn open_observed_projection(&self, decl: ObservedProjection) -> KernelEventObserverId;
    fn close_observed_projection(&self, id: KernelEventObserverId);
}

/// Declaration bundle for a single observed-projection session.
///
/// Passed to
/// [`ObservedProjectionRegistrar::open_observed_projection`]. All fields mirror
/// the parameters accepted by `open_observed_interest_pinned`; the observer is
/// registered muted and activated kernel-side after replay.
pub struct ObservedProjection {
    /// The observer that will receive kernel events for this interest.
    pub observer: Arc<dyn KernelEventObserver>,
    /// NIP-01 REQ filter JSON selecting the events for this interest.
    pub filter_json: String,
    /// Refcount owner key (unique per open screen / component).
    pub consumer_id: String,
    /// `0` = `ActiveAccount` (re-routed on account switch),
    /// `1` = `Global` (account-agnostic).
    pub scope: u32,
    /// When `Some`, pins the interest to exactly one relay (bypasses NIP-65
    /// outbox routing).  The matching close MUST pass the same pin.
    pub relay_pin: Option<String>,
    /// Shapes used during the kernel-side read-cache replay before activation.
    /// Pass `Vec::new()` to suppress replay (cache-only search, live-only tap).
    pub replay_shapes: Vec<nmp_planner::InterestShape>,
    /// Maximum number of cached events to replay before activation.
    pub replay_limit: usize,
}
