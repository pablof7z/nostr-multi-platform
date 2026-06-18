//! Generic raw signed-event forwarding — policy registration.
//!
//! The dispatcher is wired directly into the persistence chokepoint
//! (`kernel/ingest/persistence.rs`) where the typed `RawEvent` is already
//! available — zero JSON-round-trip cost.
//!
//! This module handles policy registration: building the policy list from the
//! `ExternalEventSinkPolicyFactory` and installing it on the dispatcher.

use std::sync::Arc;

use crate::kernel::Kernel;
use crate::slots::ExternalEventSinkPolicyFactory;
#[cfg(test)]
use crate::slots::ExternalEventSinkPolicySlot;
use crate::substrate::{
    ExternalEventSinkDispatcher, ExternalEventSinkPolicy, RawEventForwardPolicyContext,
};

// ─── Test-only re-registration entry point ────────────────────────────────────

#[cfg(test)]
pub(crate) fn register_raw_event_forward_policies(
    kernel: &Kernel,
    dispatcher: &ExternalEventSinkDispatcher,
    sink_policy_slot: &ExternalEventSinkPolicySlot,
) {
    let factory = sink_policy_slot
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(Arc::clone));
    register_policies_from_factories(kernel, dispatcher, factory);
}

/// Main registration path called from the actor loop on start + after Reset.
pub(crate) fn register_raw_event_forward_policies_from_factory(
    kernel: &Kernel,
    dispatcher: &ExternalEventSinkDispatcher,
    factory: Option<Arc<ExternalEventSinkPolicyFactory>>,
) {
    register_policies_from_factories(kernel, dispatcher, factory);
}

fn register_policies_from_factories(
    kernel: &Kernel,
    dispatcher: &ExternalEventSinkDispatcher,
    factory: Option<Arc<ExternalEventSinkPolicyFactory>>,
) {
    let context = RawEventForwardPolicyContext::new(
        kernel.event_store_handle(),
        kernel.indexer_relays_handle(),
    );

    let mut policies: Vec<Arc<dyn ExternalEventSinkPolicy>> = Vec::new();

    if let Some(f) = factory {
        policies.extend(f(context));
    }

    // Install policies on the dispatcher.  The dispatcher's background thread
    // resolves destinations and sends frames.
    dispatcher.set_policies(policies);
}

#[cfg(test)]
#[path = "raw_event_forwarder/tests.rs"]
mod tests;
