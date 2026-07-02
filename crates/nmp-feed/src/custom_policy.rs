//! App-registered custom feed-policy registry.
//!
//! Custom app policy enters the feed model as closed data, not closures,
//! traits, or native callbacks. The three feed phases have different contracts:
//!
//! * source ids resolve to closed [`FeedScope`] acquisition expressions;
//! * admission ids resolve to closed [`FeedScope`] expressions used as gates;
//! * order ids resolve to concrete [`FeedOrder`] values the engine must honor.
//!
//! The ids are intentionally not interchangeable. A custom source is not an
//! admission gate, and an admission gate is not an ordering policy.
//!
//! # Lifetime
//!
//! Registrations live for the lifetime of the registry. The framework hangs one
//! registry off the app. Definitions are register-once and immutable: a second
//! registration under an existing id is rejected and the first definition
//! stands. Immutability prevents fail-open drift where an already-open feed
//! keeps using an older wider policy after the registry was overwritten with a
//! narrower one.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::params::{CustomAdmissionId, CustomOrderId, CustomSourceId, FeedOrder, FeedScope};

/// Closed-data definition for an app-registered custom source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomSourceDef {
    /// The acquisition source this id expands to.
    pub source: FeedScope,
}

impl CustomSourceDef {
    #[must_use]
    pub fn new(source: FeedScope) -> Self {
        Self { source }
    }
}

/// Closed-data definition for an app-registered custom admission gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomAdmissionDef {
    /// The gate source used to narrow the already-resolved acquisition.
    pub gate: FeedScope,
}

impl CustomAdmissionDef {
    #[must_use]
    pub fn new(gate: FeedScope) -> Self {
        Self { gate }
    }
}

/// Closed-data definition for an app-registered custom order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomOrderDef {
    /// The concrete order this id expands to.
    pub order: FeedOrder,
}

impl CustomOrderDef {
    #[must_use]
    pub fn new(order: FeedOrder) -> Self {
        Self { order }
    }
}

#[derive(Default)]
struct CustomFeedPolicyDefs {
    sources: BTreeMap<CustomSourceId, CustomSourceDef>,
    admissions: BTreeMap<CustomAdmissionId, CustomAdmissionDef>,
    orders: BTreeMap<CustomOrderId, CustomOrderDef>,
}

/// Register-once app-defined feed policy definitions.
#[derive(Default)]
pub struct CustomFeedPolicyRegistry {
    defs: Mutex<CustomFeedPolicyDefs>,
}

impl CustomFeedPolicyRegistry {
    /// Register a custom source definition.
    pub fn register_source(&self, id: CustomSourceId, def: CustomSourceDef) -> bool {
        self.with_defs_mut(|defs| register_once(&mut defs.sources, id, def))
    }

    /// Register a custom admission-gate definition.
    pub fn register_admission(&self, id: CustomAdmissionId, def: CustomAdmissionDef) -> bool {
        self.with_defs_mut(|defs| register_once(&mut defs.admissions, id, def))
    }

    /// Register a custom order definition.
    pub fn register_order(&self, id: CustomOrderId, def: CustomOrderDef) -> bool {
        self.with_defs_mut(|defs| register_once(&mut defs.orders, id, def))
    }

    /// The source definition registered under `id`, or `None`.
    #[must_use]
    pub fn get_source(&self, id: &CustomSourceId) -> Option<CustomSourceDef> {
        self.defs
            .lock()
            .ok()
            .and_then(|defs| defs.sources.get(id).cloned())
    }

    /// The admission-gate definition registered under `id`, or `None`.
    #[must_use]
    pub fn get_admission(&self, id: &CustomAdmissionId) -> Option<CustomAdmissionDef> {
        self.defs
            .lock()
            .ok()
            .and_then(|defs| defs.admissions.get(id).cloned())
    }

    /// The order definition registered under `id`, or `None`.
    #[must_use]
    pub fn get_order(&self, id: &CustomOrderId) -> Option<CustomOrderDef> {
        self.defs
            .lock()
            .ok()
            .and_then(|defs| defs.orders.get(id).cloned())
    }

    #[must_use]
    pub fn is_source_registered(&self, id: &CustomSourceId) -> bool {
        self.defs
            .lock()
            .map(|defs| defs.sources.contains_key(id))
            .unwrap_or(false)
    }

    #[must_use]
    pub fn is_admission_registered(&self, id: &CustomAdmissionId) -> bool {
        self.defs
            .lock()
            .map(|defs| defs.admissions.contains_key(id))
            .unwrap_or(false)
    }

    #[must_use]
    pub fn is_order_registered(&self, id: &CustomOrderId) -> bool {
        self.defs
            .lock()
            .map(|defs| defs.orders.contains_key(id))
            .unwrap_or(false)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.defs
            .lock()
            .map(|defs| defs.sources.len() + defs.admissions.len() + defs.orders.len())
            .unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn with_defs_mut<F>(&self, f: F) -> bool
    where
        F: FnOnce(&mut CustomFeedPolicyDefs) -> bool,
    {
        self.defs
            .lock()
            .map(|mut defs| f(&mut defs))
            .unwrap_or(false)
    }
}

fn register_once<K, V>(map: &mut BTreeMap<K, V>, id: K, def: V) -> bool
where
    K: Ord,
{
    if map.contains_key(&id) {
        return false;
    }
    map.insert(id, def);
    true
}

#[cfg(test)]
#[path = "custom_policy_tests.rs"]
mod tests;
