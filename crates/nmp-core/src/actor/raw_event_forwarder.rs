//! Generic raw signed-event forwarding observer.
//!
//! This module contains only the substrate dispatch mechanics: install
//! injected policies as raw-event observers, build the `["EVENT", ...]`
//! frame, and send it through the native relay pool. Policy crates own target
//! selection and any bounded de-duplication.

use std::sync::{Arc, Mutex};

use crate::actor::{
    register_rust_raw_observer, unregister_raw_observer, RawEventObserver, RawEventObserverId,
    RawEventObserverSlot,
};
use crate::kernel::Kernel;
use crate::slots::RawEventForwardPolicySlot;
use crate::store::RawEvent;
use crate::substrate::{
    RawEventForwardPolicy, RawEventForwardPolicyContext, RawEventForwardTarget,
};

use nmp_network::pool::{Pool, WireFrame};

/// Actor-local slot containing the observer ids installed for injected
/// raw-event forwarding policies.
pub(crate) type RawEventForwardObserverIdSlot = Arc<Mutex<Vec<RawEventObserverId>>>;

#[must_use]
pub(crate) fn new_raw_event_forward_observer_id_slot() -> RawEventForwardObserverIdSlot {
    Arc::new(Mutex::new(Vec::new()))
}

pub(crate) fn register_raw_event_forward_policies(
    kernel: &Kernel,
    raw_event_observers: &RawEventObserverSlot,
    pool: &Pool,
    id_slot: &RawEventForwardObserverIdSlot,
    policy_slot: &RawEventForwardPolicySlot,
) {
    let previous_ids = id_slot
        .lock()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or_default();
    for id in previous_ids {
        unregister_raw_observer(raw_event_observers, id);
    }

    let Some(factory) = policy_slot
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(Arc::clone))
    else {
        return;
    };

    let context = RawEventForwardPolicyContext::new(
        kernel.event_store_handle(),
        kernel.indexer_relays_handle(),
    );
    let policies = factory(context);
    if policies.is_empty() {
        return;
    }

    let sender =
        Arc::new(PoolRawEventForwardSender::new(pool.clone())) as Arc<dyn RawEventForwardSender>;
    let mut new_ids = Vec::with_capacity(policies.len());
    for policy in policies {
        let kinds = policy.kind_filter();
        let observer = Arc::new(RawEventForwardObserver::new(policy, Arc::clone(&sender)))
            as Arc<dyn RawEventObserver>;
        let id = register_rust_raw_observer(raw_event_observers, kinds, observer);
        if id.0 != 0 {
            new_ids.push(id);
        }
    }

    if let Ok(mut guard) = id_slot.lock() {
        *guard = new_ids;
    }
}

pub(crate) trait RawEventForwardSender: Send + Sync {
    fn send_to(&self, target: &RawEventForwardTarget, frame_text: &str) -> bool;
}

struct PoolRawEventForwardSender {
    pool: Pool,
}

impl PoolRawEventForwardSender {
    fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl RawEventForwardSender for PoolRawEventForwardSender {
    fn send_to(&self, target: &RawEventForwardTarget, frame_text: &str) -> bool {
        let handle = self
            .pool
            .ensure_open_with_role(&target.relay_url, target.relay_role);
        self.pool
            .send(handle, WireFrame::Text(frame_text.to_string()))
    }
}

struct RawEventForwardObserver {
    policy: Arc<dyn RawEventForwardPolicy>,
    sender: Arc<dyn RawEventForwardSender>,
}

impl RawEventForwardObserver {
    fn new(policy: Arc<dyn RawEventForwardPolicy>, sender: Arc<dyn RawEventForwardSender>) -> Self {
        Self { policy, sender }
    }

    fn process(
        &self,
        raw: &RawEvent,
        source_relay_url: Option<&str>,
        verbatim_json: &str,
    ) -> usize {
        let targets = self.policy.forward_targets(raw, source_relay_url);
        if targets.is_empty() {
            return 0;
        }
        let frame_text = format!(r#"["EVENT",{verbatim_json}]"#);
        let mut sent = 0usize;
        for target in &targets {
            if self.sender.send_to(target, &frame_text) {
                sent = sent.saturating_add(1);
            }
        }
        sent
    }
}

impl RawEventObserver for RawEventForwardObserver {
    fn on_raw_event(&self, _kind: u32, _json: &str) {}

    fn on_raw_event_with_source(&self, _kind: u32, json: &str, source_relay_url: Option<&str>) {
        let Ok(raw) = serde_json::from_str::<RawEvent>(json) else {
            return;
        };
        let _ = self.process(&raw, source_relay_url, json);
    }
}

#[cfg(test)]
#[path = "raw_event_forwarder/tests.rs"]
mod tests;
