use std::collections::BTreeMap;

use nmp_core::substrate::KernelEvent;
use nmp_kinds::KIND_SHORT_TEXT_NOTE;
use nmp_nip18::{
    try_from_kernel_event as repost_from_kernel_event, KIND_DELETE, KIND_GENERIC_REPOST,
    KIND_REPOST,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::target::RepostTarget;

/// The single NIP-18 repost demand for one target: kind:6 (implicit kind:1
/// target) and kind:16 (generic repost, `k`-tag discriminated) reposts, plus
/// kind:5 NIP-09 deletes so a reposter's own retraction is observable on the
/// same live subscription (best-effort: NIP-09 names the retracted event's
/// own id in its `e` tags, not the target's, so this catches only a delete
/// that also happens to co-tag the target — see [`crate::summary`] docs).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RepostReadPlan {
    target_event_id: String,
}

impl RepostReadPlan {
    #[must_use]
    pub fn new(target: &RepostTarget) -> Self {
        Self {
            target_event_id: target.event_id().to_string(),
        }
    }

    /// The routed `REQ` filter for this plan's demand.
    #[must_use]
    pub fn filter_json(&self) -> String {
        // BTreeMap serializes keys in sorted order regardless of serde_json's
        // `preserve_order` feature, so the filter string is deterministic
        // across workspace feature-unification (mirrors nmp-replies).
        let mut map: BTreeMap<String, Value> = BTreeMap::new();
        map.insert(
            "kinds".to_string(),
            Value::Array(
                [KIND_REPOST, KIND_GENERIC_REPOST, KIND_DELETE]
                    .into_iter()
                    .map(Value::from)
                    .collect(),
            ),
        );
        map.insert(
            "#e".to_string(),
            Value::Array(vec![Value::String(self.target_event_id.clone())]),
        );
        serde_json::to_string(&map).expect("filter map serializes")
    }

    /// Whether `event` is an ACCEPTED repost of the target, returning the
    /// reposter's raw pubkey when it is.
    ///
    /// Admission: a kind:6 wrapper (implicit kind:1 target) or a kind:16
    /// generic repost whose `k` tag names kind:1 (NIP-18 target-kind
    /// discrimination) — never a repost of a different target-kind that
    /// merely happens to `#e`-tag this id. The demand filter is a superset;
    /// this narrows it, exactly like `nmp-replies`' `accepts_nip10`.
    #[must_use]
    pub fn accepts_repost(&self, event: &KernelEvent) -> Option<String> {
        let record = repost_from_kernel_event(event)?;
        if record.target_event_id.as_deref() != Some(self.target_event_id.as_str()) {
            return None;
        }
        match record.target_kind {
            None | Some(KIND_SHORT_TEXT_NOTE) => Some(record.author),
            Some(_) => None,
        }
    }

    /// The target event id this plan reads reposts for.
    #[must_use]
    pub fn target_event_id(&self) -> &str {
        &self.target_event_id
    }
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
