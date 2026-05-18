//! `PublishPlan` and `RelayPin` — the typed carrier the publish planner
//! consults for routing.
//!
//! Per `routing.md` §5: every NIP-29 action emits a `PublishPlan` with a
//! `Some(RelayPin)` set by the action that knows the host. The planner
//! consults this typed field for routing; it does NOT inspect event tags to
//! *derive* routing. The planner's only tag inspection is a defensive refusal
//! when an event carries `["h", _]` and `pin_to: None` (the
//! `MissingHostPinForGroupEvent` error per routing.md §5).
//!
//! This module is the typed carrier; the planner integration lands when M2's
//! publish planner is implemented (M6 + this milestone) — for M11.5 Step 0
//! the carrier is the contract the actions agree to.

use serde::{Deserialize, Serialize};

use crate::group_id::{GroupId, RelayUrl};

/// Routing pin: a single relay URL the publish must target exclusively.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RelayPin {
    pub relay_url: RelayUrl,
    /// Carries the typed `GroupId` for diagnostics + audit; the planner uses
    /// `relay_url` for routing and ignores this.
    pub source_group: Option<GroupId>,
}

impl RelayPin {
    pub fn for_group(group: &GroupId) -> Self {
        Self {
            relay_url: group.host_relay_url.clone(),
            source_group: Some(group.clone()),
        }
    }
}

/// Pre-signing publish plan: the unsigned event shape + the typed routing
/// pin. The actor's signer-bridge converts this to a signed event and the
/// planner dispatches via the pin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublishPlan {
    pub kind: u32,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    /// Host-relay-pin per routing.md §5. Always `Some(_)` for NIP-29 actions.
    pub pin_to: Option<RelayPin>,
}

impl PublishPlan {
    pub fn pinned(group: &GroupId, kind: u32, content: impl Into<String>, tags: Vec<Vec<String>>) -> Self {
        Self {
            kind,
            content: content.into(),
            tags,
            pin_to: Some(RelayPin::for_group(group)),
        }
    }

    /// Defensive structural check the planner performs at construction time
    /// per routing.md §5: any event carrying `["h", _]` MUST have
    /// `pin_to.is_some()`, otherwise reject with `MissingHostPinForGroupEvent`.
    /// This is the privacy-leak prevention guard.
    pub fn validate_no_unpinned_h(&self) -> Result<(), PublishPlanError> {
        let has_h = self.tags.iter().any(|t| t.len() >= 2 && t[0] == "h");
        if has_h && self.pin_to.is_none() {
            return Err(PublishPlanError::MissingHostPinForGroupEvent);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PublishPlanError {
    MissingHostPinForGroupEvent,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> GroupId { GroupId::new("wss://h.example.com", "room") }

    #[test]
    fn pinned_carries_host_relay() {
        let p = PublishPlan::pinned(&g(), 9, "hi", vec![vec!["h".into(), "room".into()]]);
        let pin = p.pin_to.unwrap();
        assert_eq!(pin.relay_url, "wss://h.example.com");
        assert_eq!(pin.source_group.unwrap(), g());
    }

    #[test]
    fn validate_rejects_unpinned_h_tag() {
        let mut p = PublishPlan::pinned(&g(), 9, "hi", vec![vec!["h".into(), "room".into()]]);
        p.pin_to = None;
        assert_eq!(
            p.validate_no_unpinned_h().unwrap_err(),
            PublishPlanError::MissingHostPinForGroupEvent
        );
    }

    #[test]
    fn validate_passes_pinned_h_tag() {
        let p = PublishPlan::pinned(&g(), 9, "hi", vec![vec!["h".into(), "room".into()]]);
        assert!(p.validate_no_unpinned_h().is_ok());
    }

    #[test]
    fn validate_passes_no_h_tag() {
        let p = PublishPlan {
            kind: 1,
            content: "public".into(),
            tags: vec![],
            pin_to: None,
        };
        assert!(p.validate_no_unpinned_h().is_ok());
    }
}
