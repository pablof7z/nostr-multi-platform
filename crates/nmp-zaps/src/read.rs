//! Zap read plan — compiles the NIP-57 kind:9735 `#e` demand for one
//! [`ZapTarget`] and applies admission (the adapter's own receipt validation
//! plus a target match). Protocol semantics — receipt decode, bolt11/embedded
//! amount consistency, and provider-mismatch rejection — stay in
//! `nmp-nip57`; this plan never re-parses a tag or a bolt11 invoice.

use std::collections::BTreeMap;

use nmp_core::substrate::{KernelEvent, ViewDependencies};
use nmp_kinds::KIND_ZAP_RECEIPT;
use nmp_nip57::{try_from_kernel_event_validated, ZapReceiptRecord};
use serde_json::Value;

use crate::target::ZapTarget;

#[derive(Clone, Debug, PartialEq)]
pub struct ZapReadPlan {
    target: ZapTarget,
    dependencies: ViewDependencies,
}

impl ZapReadPlan {
    #[must_use]
    pub fn new(target: ZapTarget) -> Self {
        let dependencies = ViewDependencies {
            kinds: vec![KIND_ZAP_RECEIPT],
            tag_refs: vec![("e".to_string(), target.event_id().to_string())],
            ..Default::default()
        };
        Self {
            target,
            dependencies,
        }
    }

    #[must_use]
    pub fn target(&self) -> &ZapTarget {
        &self.target
    }

    #[must_use]
    pub fn filter_json(&self) -> String {
        // BTreeMap serializes keys in sorted order regardless of serde_json's
        // `preserve_order` feature, so the filter string is deterministic
        // across workspace feature-unification.
        let mut map: BTreeMap<String, Value> = BTreeMap::new();
        map.insert(
            "kinds".to_string(),
            Value::Array(
                self.dependencies
                    .kinds
                    .iter()
                    .map(|kind| Value::from(*kind))
                    .collect(),
            ),
        );
        for (tag, value) in &self.dependencies.tag_refs {
            map.insert(
                format!("#{tag}"),
                Value::Array(vec![Value::String(value.clone())]),
            );
        }
        serde_json::to_string(&map).expect("filter map serializes")
    }

    /// Admission: decode `event` through the adapter's own validation
    /// (`nmp_nip57::try_from_kernel_event_validated` — amount consistency +
    /// known-provider mismatch rejection) and accept it only when the
    /// decoded receipt zapped this plan's target. Returns the decoded
    /// receipt on acceptance so the reducer never re-decodes.
    #[must_use]
    pub fn accepts(&self, event: &KernelEvent) -> Option<ZapReceiptRecord> {
        let record = try_from_kernel_event_validated(event)?;
        if record.zapped_event_id.as_deref() != Some(self.target.event_id()) {
            return None;
        }
        Some(record)
    }
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
