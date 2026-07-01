//! `GenericOutboxRouter` — the single
//! [`nmp_core::substrate::OutboxRouter`] impl
//! (`docs/architecture/crate-boundaries.md` §3.2).
//!
//! Lanes implemented (spec §3.1):
//!
//! - **Lane 1 — NIP-65 mailbox.** `route_publish` consults
//!   [`MailboxCache::write_relays`] for `evt.pubkey`; `route_subscription`
//!   consults `read_relays` for each author in the interest shape.
//! - **Lane 2 — Hint.** Relay-hint URLs lifted from `evt.tags`
//!   (e/p/a/q tag position 2) on publish; lifted from `interest.hints`
//!   carrying [`HintSource::EventTag`] on subscribe. Stacks on top of
//!   lane 1 — never substitutes.
//! - **Lane 3 — Provenance.** Subscribe-only: lifted from
//!   `interest.hints` carrying [`HintSource::Provenance`] (the relay we
//!   last saw a referenced event id at, so a re-fetch goes back there).
//! - **Lane 4 — UserConfigured.** When `evt.pubkey == ctx.active_account`
//!   on publish, `session_keys.active_write` is attributed to
//!   [`UserConfiguredCategory::ActiveAccountWrite`]; when an author in the
//!   interest shape (or the active account itself for an authorless
//!   wildcard) matches `ctx.active_account` on subscribe,
//!   `session_keys.active_read` is attributed to
//!   [`UserConfiguredCategory::ActiveAccountRead`].
//! - **Lane 6 — Indexer.** ALWAYS-ON for discovery kinds (kind:0,
//!   kind:3, kind:10000–19999) — both publish and subscribe (R+W
//!   symmetric per spec §3.1). Stacks on top of lane 1; defeats the
//!   kind:10002 self-sealing loop.
//! - **Lane 7 — AppRelay.** Fallback when no earlier lane resolved
//!   anything.
//!
//! Blocked-relay policy is a subtractive post-filter applied via per-lane
//! `blocked_relays.contains` guards. The router consumes the generic lookup
//! result and never parses the source wire artifact.

use std::sync::Arc;

use nmp_core::substrate::{
    OutboxRouter, RoutedRelaySet, RoutingContext, RoutingError, RoutingTraceObserver,
};
use nmp_planner::LogicalInterest;
use nmp_signer_iface::UnsignedEvent;

use crate::relay_admission::{PrivateNetworkPolicy, RelayAdmissionPolicy};
mod publish;
mod subscription;

#[cfg(test)]
use nmp_core::substrate::{
    AppRelayMode, Direction, PublishTrace, RoutingSource, SubscriptionTrace, UserConfiguredCategory,
};

pub struct GenericOutboxRouter {
    /// V-51 phase 1 — optional trace observer fired after every successful
    /// `route_publish` / `route_subscription`. `None` by default; production
    /// composition binds the kernel's `RoutingTraceProjection` clone via
    /// [`Self::with_trace_observer`]. D8: the `Option::is_some` gate keeps
    /// the no-observer path zero-alloc.
    trace_observer: Option<Arc<dyn RoutingTraceObserver>>,
    /// Admission policy applied to **untrusted lanes 1–3** (NIP-65 mailbox,
    /// event-tag hints, provenance). Operator-controlled lanes (4, 6, 7) are
    /// exempt so local dev relays work as configured. Default is
    /// [`PrivateNetworkPolicy`].
    admission: Arc<dyn RelayAdmissionPolicy>,
}

impl Default for GenericOutboxRouter {
    fn default() -> Self {
        Self {
            trace_observer: None,
            admission: Arc::new(PrivateNetworkPolicy),
        }
    }
}

impl GenericOutboxRouter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a [`RoutingTraceObserver`] (V-51 phase 1). The router fires
    /// `on_publish` / `on_subscription` after every successful resolution;
    /// `Err(RoutingError::*)` returns are NOT observed.
    #[must_use]
    pub fn with_trace_observer(mut self, obs: Arc<dyn RoutingTraceObserver>) -> Self {
        self.trace_observer = Some(obs);
        self
    }

    /// Override the relay admission policy for untrusted lanes (1–3). The
    /// default is [`PrivateNetworkPolicy`]; pass a custom impl to extend or
    /// replace it (e.g. an operator deny-list composed with the private-network
    /// check).
    #[must_use]
    pub fn with_admission_policy(mut self, policy: Arc<dyn RelayAdmissionPolicy>) -> Self {
        self.admission = policy;
        self
    }
}

impl OutboxRouter for GenericOutboxRouter {
    fn route_publish(
        &self,
        evt: &UnsignedEvent,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        publish::route(self, evt, ctx)
    }

    fn route_subscription(
        &self,
        interest: &LogicalInterest,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        subscription::route(self, interest, ctx)
    }
}

#[cfg(test)]
#[path = "router/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "router/tests_lanes.rs"]
mod tests_lanes;

#[cfg(test)]
#[path = "router/tests_v75.rs"]
mod tests_v75;

#[cfg(test)]
#[path = "router/tests_v52.rs"]
mod tests_v52;

#[cfg(test)]
#[path = "router/tests_indexer_scope.rs"]
mod tests_indexer_scope;
