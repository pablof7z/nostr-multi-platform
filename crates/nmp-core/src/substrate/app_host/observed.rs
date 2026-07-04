//! Observed-projection registration seam.
//!
//! Split out of `app_host/mod.rs` (file-size ceiling, AGENTS.md). This module
//! owns the declaration bundle a host needs before a read model may receive
//! accepted events.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::actor::{
    register_rust_observer_muted, unregister_observer_internal, ActorCommand, InterestsCommand,
    ObservedProjectionSinkSlot,
};
use crate::CommandSender;
use crate::{ObservedProjectionId, ObservedProjectionSink};
use nmp_planner::{InterestLifecycle, InterestShape};

/// Session metadata needed to reverse an observed-projection open.
pub type ObservedProjectionSessionMap =
    Arc<Mutex<HashMap<ObservedProjectionId, (String, String, u32, Option<String>, bool)>>>;

/// Cloneable command-backed observed-projection registrar.
///
/// Runtime controllers cannot keep borrowing the composition root after
/// startup. This handle carries only the observer slot, close-session map, and
/// command sender needed to open/close declared observed projections through
/// the same `OpenObservedInterest` path.
#[derive(Clone)]
pub struct ObservedProjectionCommandHandle {
    observers: ObservedProjectionSinkSlot,
    sessions: ObservedProjectionSessionMap,
    sender: CommandSender,
}

impl ObservedProjectionCommandHandle {
    #[must_use]
    pub fn new(
        observers: ObservedProjectionSinkSlot,
        sessions: ObservedProjectionSessionMap,
        sender: CommandSender,
    ) -> Self {
        Self {
            observers,
            sessions,
            sender,
        }
    }

    /// Open a declared observed projection.
    #[must_use]
    pub fn open(&self, decl: ObservedProjection) -> ObservedProjectionId {
        if !decl.has_declared_shape() {
            return ObservedProjectionId(0);
        }
        self.open_with_replay(decl, true)
    }

    /// Open a declared observed projection without generic read-cache replay.
    ///
    /// This is intentionally narrow. Most read models must use [`Self::open`],
    /// whose non-empty replay-shape guard prevents late-joiner data loss.
    /// Protocols with a stronger cache path, such as NIP-50 FTS search, use
    /// this live-only variant so stale structural cache replay cannot bypass
    /// their own query filter.
    #[must_use]
    pub fn open_live_only(&self, decl: ObservedProjection) -> ObservedProjectionId {
        if filter_json_is_empty(&decl.filter_json)
            || InterestShape::from_filter_json(&decl.filter_json).is_none()
        {
            return ObservedProjectionId(0);
        }
        self.open_with_replay(decl, false)
    }

    fn open_with_replay(&self, mut decl: ObservedProjection, replay: bool) -> ObservedProjectionId {
        if !replay {
            decl.replay_shapes.clear();
            decl.replay_limit = 0;
        }
        let observer_id = register_rust_observer_muted(&self.observers, decl.observer);
        if observer_id.0 == 0 {
            return observer_id;
        }
        let Ok(mut sessions) = self.sessions.lock() else {
            unregister_observer_internal(&self.observers, observer_id);
            return ObservedProjectionId(0);
        };
        sessions.insert(
            observer_id,
            (
                decl.filter_json.clone(),
                decl.consumer_id.clone(),
                decl.scope,
                decl.relay_pin.clone(),
                decl.is_indexer_discovery,
            ),
        );
        drop(sessions);
        self.open_observed_interest_pinned(
            &decl.filter_json,
            &decl.consumer_id,
            decl.scope,
            decl.relay_pin,
            decl.is_indexer_discovery,
            decl.lifecycle,
            observer_id,
            decl.replay_shapes,
            decl.replay_limit,
        );
        observer_id
    }

    /// Close an observed projection previously opened by [`Self::open`].
    pub fn close(&self, id: ObservedProjectionId) {
        let params = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(&id));
        let Some((filter_json, consumer_id, scope, relay_pin, is_indexer_discovery)) = params
        else {
            return;
        };
        self.close_interest_pinned(
            &filter_json,
            &consumer_id,
            scope,
            relay_pin,
            is_indexer_discovery,
        );
        unregister_observer_internal(&self.observers, id);
    }

    #[allow(clippy::too_many_arguments)]
    fn open_observed_interest_pinned(
        &self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
        relay_pin: Option<String>,
        is_indexer_discovery: bool,
        lifecycle: InterestLifecycle,
        observer_id: ObservedProjectionId,
        replay_shapes: Vec<InterestShape>,
        replay_limit: usize,
    ) {
        if InterestShape::from_filter_json(filter_json).is_none() {
            return;
        }
        let _ = self.sender.send(ActorCommand::Interests(
            InterestsCommand::OpenObservedInterest {
                filter_json: filter_json.to_string(),
                consumer_id: consumer_id.to_string(),
                scope,
                relay_pin,
                is_indexer_discovery,
                lifecycle,
                observer_id,
                replay_shapes,
                replay_limit,
            },
        ));
    }

    fn close_interest_pinned(
        &self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
        relay_pin: Option<String>,
        is_indexer_discovery: bool,
    ) {
        let _ = self
            .sender
            .send(ActorCommand::Interests(InterestsCommand::CloseInterest {
                filter_json: filter_json.to_string(),
                consumer_id: consumer_id.to_string(),
                scope,
                relay_pin,
                is_indexer_discovery,
            }));
    }
}

impl ObservedProjectionRegistrar for ObservedProjectionCommandHandle {
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.open(decl)
    }

    fn close_observed_projection(&self, id: ObservedProjectionId) {
        self.close(id);
    }

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        Arc::new(self.clone())
    }
}

/// Register and close **observed projections**.
///
/// [`open_observed_projection`](ObservedProjectionRegistrar::open_observed_projection)
/// combines observer registration (muted), an interest open, and a
/// kernel-side muted→activate replay sequence in a single call, so the
/// observer cannot miss matching events that arrived before it was registered.
/// [`close_observed_projection`](ObservedProjectionRegistrar::close_observed_projection)
/// reverses both registrations atomically.
pub trait ObservedProjectionRegistrar {
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId;
    fn close_observed_projection(&self, id: ObservedProjectionId);

    /// Return a cloneable app-lifetime registrar handle for reconciler
    /// callbacks that run after registration time.
    ///
    /// Runtime controllers use this instead of capturing `&self` into
    /// `'static` tick observers. The handle must call the same open/close
    /// implementation as this registrar.
    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync>;
}

/// Declaration bundle for a single observed-projection session.
///
/// Passed to
/// [`ObservedProjectionRegistrar::open_observed_projection`]. All fields mirror
/// the parameters accepted by `open_observed_interest_pinned`; the observer is
/// registered muted and activated kernel-side after replay.
pub struct ObservedProjection {
    /// The observer that will receive kernel events for this interest.
    pub observer: Arc<dyn ObservedProjectionSink>,
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
    /// Route this sparse global read through indexer-discovery relays instead
    /// of the normal content/outbox lane.
    pub is_indexer_discovery: bool,
    /// #2948 — close semantics for the compiled REQ. Defaults to
    /// [`InterestLifecycle::Tailing`] (stay live after EOSE) for every existing
    /// read model; a concept that wants a collection that completes on EOSE
    /// opts into [`InterestLifecycle::OneShot`] via [`Self::with_lifecycle`].
    pub lifecycle: InterestLifecycle,
    /// Shapes used during the kernel-side read-cache replay before activation
    /// and for scoped future delivery. This must be non-empty for production
    /// read models.
    pub replay_shapes: Vec<InterestShape>,
    /// Maximum number of cached events to replay before activation.
    pub replay_limit: usize,
}

impl ObservedProjection {
    /// Build a declaration from the same shape used for future delivery.
    ///
    /// This is the common case for production read models: a concrete
    /// `InterestShape` is converted into the wire filter, replay selector, and
    /// future-delivery selector in one place.
    #[must_use]
    pub fn from_shape(
        observer: Arc<dyn ObservedProjectionSink>,
        consumer_id: impl Into<String>,
        scope: u32,
        shape: InterestShape,
        replay_limit: usize,
    ) -> Self {
        let filter_json = crate::subs::wire::filter_json_for(&shape);
        let relay_pin = shape.relay_pin.clone();
        Self {
            observer,
            filter_json,
            consumer_id: consumer_id.into(),
            scope,
            relay_pin,
            is_indexer_discovery: false,
            lifecycle: InterestLifecycle::Tailing,
            replay_shapes: vec![shape],
            replay_limit,
        }
    }

    /// Build a declaration scoped only by event kind.
    #[must_use]
    pub fn from_kinds<I>(
        observer: Arc<dyn ObservedProjectionSink>,
        consumer_id: impl Into<String>,
        scope: u32,
        kinds: I,
        replay_limit: usize,
    ) -> Self
    where
        I: IntoIterator<Item = u32>,
    {
        Self::from_shape(
            observer,
            consumer_id,
            scope,
            InterestShape {
                kinds: kinds.into_iter().collect(),
                ..Default::default()
            },
            replay_limit,
        )
    }

    /// Opt this observed projection into indexer-discovery routing.
    #[must_use]
    pub fn with_indexer_discovery(mut self, enabled: bool) -> Self {
        self.is_indexer_discovery = enabled;
        self
    }

    /// #2948 — select the compiled REQ's close semantics. Defaults to
    /// [`InterestLifecycle::Tailing`]; a collection read that should complete
    /// and tear down on EOSE passes [`InterestLifecycle::OneShot`].
    #[must_use]
    pub fn with_lifecycle(mut self, lifecycle: InterestLifecycle) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    /// Whether the declaration has a concrete event shape.
    ///
    /// Production hosts reject declarations with no shape, a `{}` filter, or
    /// an all-empty replay shape because that recreates the deleted filterless
    /// observer lane.
    #[must_use]
    pub fn has_declared_shape(&self) -> bool {
        if self.replay_shapes.is_empty() || filter_json_is_empty(&self.filter_json) {
            return false;
        }
        self.replay_shapes.iter().all(shape_has_predicate)
    }
}

fn filter_json_is_empty(filter_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(filter_json)
        .ok()
        .and_then(|value| value.as_object().map(|object| object.is_empty()))
        .unwrap_or(true)
}

fn shape_has_predicate(shape: &InterestShape) -> bool {
    !shape.authors.is_empty()
        || !shape.kinds.is_empty()
        || !shape.tags.is_empty()
        || shape.since.is_some()
        || shape.until.is_some()
        || shape.limit.is_some()
        || shape.search.is_some()
        || !shape.event_ids.is_empty()
        || !shape.addresses.is_empty()
        || shape.relay_pin.is_some()
}
