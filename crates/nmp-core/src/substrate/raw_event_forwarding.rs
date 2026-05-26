//! Raw signed-event forwarding contract.
//!
//! `nmp-core` owns the transport-adjacent dispatch seam: after an inbound
//! signed event has passed verification and store insertion, the actor can
//! ask an injected policy which relay targets should receive the same signed
//! event frame. The policy decides *whether* and *where* to forward; the
//! actor owns the `Pool` send because sockets are substrate runtime state.

use std::sync::Arc;

use crate::slots::IndexerRelaysSlot;
use crate::store::{EventStore, RawEvent};
use crate::{KindFilter, RelayRole};

/// Kernel-owned handles available to a raw-event forwarding policy.
///
/// The fields are reader handles only. The actor remains the sole writer of
/// the relay slots, and the store remains the durable provenance source.
#[derive(Clone)]
pub struct RawEventForwardPolicyContext {
    pub event_store: Arc<dyn EventStore>,
    pub indexer_relays: IndexerRelaysSlot,
}

impl RawEventForwardPolicyContext {
    #[must_use]
    pub fn new(event_store: Arc<dyn EventStore>, indexer_relays: IndexerRelaysSlot) -> Self {
        Self {
            event_store,
            indexer_relays,
        }
    }
}

/// One resolved relay target for a forwarded signed event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawEventForwardTarget {
    pub relay_url: String,
    pub relay_role: RelayRole,
}

impl RawEventForwardTarget {
    #[must_use]
    pub fn new(relay_url: String, relay_role: RelayRole) -> Self {
        Self {
            relay_url,
            relay_role,
        }
    }
}

/// Policy object injected by reusable crates.
///
/// Implementations should be cheap, deterministic, and side-effect free
/// except for their own bounded in-memory bookkeeping. Returning an empty
/// target list means "do not forward this event".
pub trait RawEventForwardPolicy: Send + Sync {
    /// Event kinds this policy wants to observe. Empty means all kinds.
    fn kind_filter(&self) -> KindFilter;

    /// Resolve forwarding targets for `raw`, given the relay that delivered
    /// it. The actor will wrap the event JSON in `["EVENT", ...]` and send it
    /// to each returned target.
    fn forward_targets(
        &self,
        raw: &RawEvent,
        source_relay_url: Option<&str>,
    ) -> Vec<RawEventForwardTarget>;
}
