//! Raw signed-event forwarding shared data types.
//!
//! `RawEventForwardPolicyContext` and `RawEventForwardTarget` are the shared
//! data types used by the external event sink policy path. The policy trait
//! itself is `ExternalEventSinkPolicy` in `external_event_sink/mod.rs`.

use std::sync::Arc;

use crate::slots::IndexerRelaysSlot;
use crate::store::EventStore;
use crate::relay::RelayRole;

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
