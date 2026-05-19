//! `PublishPlan` + `RelayPin` — the typed carrier the publish planner
//! consults for routing. Mirrors `nmp-nip29::action::publish_plan`.
//!
//! Per `docs/plan/marmot-mls.md` §Step 4 + ADR-0012: Marmot group events
//! (kind:445) are relay-pinned to the group relay. The ActionModule emits the
//! UNSIGNED event shape + a `RelayPin`; the actor's signer-bridge signs and
//! the planner routes via the `relay_pin` lane. KeyPackage events
//! (kind:30443/443) are NOT pinned — they use standard author-write outbox
//! routing (`pin_to: None`).

use serde::{Deserialize, Serialize};

/// Routing pin: a single group relay URL the publish must target exclusively.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RelayPin {
    pub relay_url: String,
    /// Hex MLS group id for diagnostics / audit; the planner uses `relay_url`
    /// for routing and ignores this.
    pub source_group_id_hex: Option<String>,
}

impl RelayPin {
    pub fn for_group(group_relay_url: impl Into<String>, group_id_hex: impl Into<String>) -> Self {
        Self {
            relay_url: group_relay_url.into(),
            source_group_id_hex: Some(group_id_hex.into()),
        }
    }
}

/// Pre-signing publish plan: the unsigned event shape + the typed routing
/// pin. The actor's signer-bridge converts this to a signed event and the
/// planner dispatches via the pin (or via standard outbox when `pin_to` is
/// `None`, e.g. KeyPackages).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublishPlan {
    pub kind: u32,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    /// Group-relay-pin per ADR-0012. `Some(_)` for kind:445 group events;
    /// `None` for kind:30443/443 KeyPackages (standard author-write outbox).
    pub pin_to: Option<RelayPin>,
}

impl PublishPlan {
    /// A relay-pinned plan for a kind:445 group event.
    pub fn pinned(
        group_relay_url: impl Into<String>,
        group_id_hex: impl Into<String>,
        kind: u32,
        content: impl Into<String>,
        tags: Vec<Vec<String>>,
    ) -> Self {
        Self {
            kind,
            content: content.into(),
            tags,
            pin_to: Some(RelayPin::for_group(group_relay_url, group_id_hex)),
        }
    }

    /// An un-pinned plan for a KeyPackage (standard author-write outbox).
    pub fn outbox(kind: u32, content: impl Into<String>, tags: Vec<Vec<String>>) -> Self {
        Self {
            kind,
            content: content.into(),
            tags,
            pin_to: None,
        }
    }

    /// Defensive structural check: any kind:445 group event MUST be pinned.
    /// Mirrors nmp-nip29's `MissingHostPinForGroupEvent` privacy guard —
    /// publishing a group event to non-pinned relays leaks group activity.
    pub fn validate_group_event_pinned(&self) -> Result<(), PublishPlanError> {
        if self.kind == crate::interest::KIND_GROUP_MESSAGE && self.pin_to.is_none() {
            return Err(PublishPlanError::MissingGroupRelayPin);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PublishPlanError {
    MissingGroupRelayPin,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interest::{KIND_GROUP_MESSAGE, KIND_KEY_PACKAGE};

    #[test]
    fn pinned_carries_group_relay() {
        let p = PublishPlan::pinned(
            "wss://g.example.com",
            "abcd",
            KIND_GROUP_MESSAGE,
            "ct",
            vec![],
        );
        let pin = p.pin_to.clone().unwrap();
        assert_eq!(pin.relay_url, "wss://g.example.com");
        assert_eq!(pin.source_group_id_hex.unwrap(), "abcd");
        assert!(p.validate_group_event_pinned().is_ok());
    }

    #[test]
    fn unpinned_group_event_rejected() {
        let mut p = PublishPlan::pinned(
            "wss://g.example.com",
            "abcd",
            KIND_GROUP_MESSAGE,
            "ct",
            vec![],
        );
        p.pin_to = None;
        assert_eq!(
            p.validate_group_event_pinned().unwrap_err(),
            PublishPlanError::MissingGroupRelayPin
        );
    }

    #[test]
    fn key_package_outbox_unpinned_ok() {
        let p = PublishPlan::outbox(KIND_KEY_PACKAGE, "kp", vec![]);
        assert!(p.pin_to.is_none());
        assert!(p.validate_group_event_pinned().is_ok());
    }
}
