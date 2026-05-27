//! Substrate view types: the typed contract between NIP crates and the kernel's
//! subscription planner.
//!
//! `ViewDependencies` is the primary bridge type. A NIP crate's view module
//! calls [`ViewDependencies::into_logical_interest`] to convert its declared
//! event needs (kinds, authors, tag-refs, relay-pin, limit) into a
//! [`crate::planner::LogicalInterest`] suitable for `NmpApp::push_interest`.
//!
//! `KernelEvent` is the substrate-level event representation passed to
//! `KernelEventObserver` implementations — it carries only the fields the
//! kernel guarantees across all protocols (no NIP-specific payload parsing).

use serde::{Deserialize, Serialize};

use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use std::collections::{BTreeMap, BTreeSet};

pub type EventId = String;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct KernelEvent {
    pub id: EventId,
    pub author: String,
    pub kind: u32,
    pub created_at: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ViewDependencies {
    pub kinds: Vec<u32>,
    pub authors: Vec<String>,
    pub ids: Vec<EventId>,
    pub tag_refs: Vec<(String, String)>,
    pub projection_keys: Vec<String>,
    /// Host-relay this view's interest must be pinned to (e.g. single-group
    /// views pinned to a specific relay). `None` means the standard outbox/inbox
    /// routing applies.
    pub relay_pin: Option<String>,
    /// Maximum number of historical events to fetch per REQ subscription.
    /// Maps directly to [`InterestShape::limit`]. `None` means no client-side
    /// limit (relay applies its own default). Use for bounded lookups such as
    /// `KeyPackage` fetch (`limit: Some(4)`) or chat history (`limit: Some(200)`).
    pub limit: Option<u32>,
}

impl ViewDependencies {
    /// Convert this dependency declaration into a `LogicalInterest` suitable
    /// for `NmpApp::push_interest`. This is the canonical bridge between the
    /// substrate view contract and the planner's routing layer.
    ///
    /// `id` — a stable, deterministic `InterestId` (hash the namespace + key
    ///   discriminant so the same view always produces the same id; idempotent
    ///   re-registration de-dupes in the kernel).
    ///
    /// `scope` — `InterestScope::Account(pubkey)` for inbox-style subscriptions
    ///   tied to a specific account's mailbox relays; `InterestScope::Global` for
    ///   relay-pinned or author-set subscriptions. Relay-pinned interests MUST
    ///   use `Global` — the `relay_pin` field routes them to the right relay.
    ///
    /// `lifecycle` — `Tailing` for live subscriptions, `OneShot` for historical
    ///   fetch-and-close requests.
    #[must_use]
    pub fn into_logical_interest(
        &self,
        id: InterestId,
        scope: InterestScope,
        lifecycle: InterestLifecycle,
    ) -> LogicalInterest {
        let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (key, val) in &self.tag_refs {
            tags.entry(key.clone()).or_default().insert(val.clone());
        }
        LogicalInterest {
            id,
            scope,
            shape: InterestShape {
                kinds: self.kinds.iter().copied().collect(),
                authors: self.authors.iter().cloned().collect(),
                event_ids: self.ids.iter().cloned().collect(),
                tags,
                relay_pin: self.relay_pin.clone(),
                limit: self.limit,
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle,
            // View-module interests are reactive readers; they ride NIP-65
            // outbox routing and never want the indexer fallback.
            is_indexer_discovery: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProjectionChange {
    pub namespace: String,
    pub key: String,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug, Default)]
pub struct ViewContext {}

// Per-protocol view types (`RepliesView`, `Nip10ModularTimelineView`, …) expose
// their `open` / `on_event_*` / `snapshot` methods as plain inherent methods
// reached via static dispatch. `ViewDependencies` below is the load-bearing
// bridge from a view's event needs to the planner's `LogicalInterest`.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{InterestId, InterestLifecycle, InterestScope};

    #[test]
    fn bridge_maps_kinds_and_relay_pin() {
        let deps = ViewDependencies {
            kinds: vec![445],
            relay_pin: Some("wss://group.relay/".to_string()),
            ..Default::default()
        };
        let interest = deps.into_logical_interest(
            InterestId(42),
            InterestScope::Global,
            InterestLifecycle::Tailing,
        );
        assert!(interest.shape.kinds.contains(&445));
        assert_eq!(
            interest.shape.relay_pin.as_deref(),
            Some("wss://group.relay/")
        );
        assert_eq!(interest.id, InterestId(42));
        assert!(matches!(interest.scope, InterestScope::Global));
        assert!(matches!(interest.lifecycle, InterestLifecycle::Tailing));
    }

    #[test]
    fn bridge_maps_tag_refs_to_btreemap() {
        let deps = ViewDependencies {
            kinds: vec![1059],
            tag_refs: vec![("p".to_string(), "pubkey123".to_string())],
            ..Default::default()
        };
        let interest = deps.into_logical_interest(
            InterestId(99),
            InterestScope::Account("pubkey123".to_string()),
            InterestLifecycle::Tailing,
        );
        let p_tags = interest.shape.tags.get("p").expect("p tag must be present");
        assert!(p_tags.contains("pubkey123"));
        assert!(matches!(interest.scope, InterestScope::Account(ref pk) if pk == "pubkey123"));
    }

    #[test]
    fn bridge_maps_authors() {
        let deps = ViewDependencies {
            kinds: vec![30443, 443],
            authors: vec!["author_pubkey".to_string()],
            ..Default::default()
        };
        let interest = deps.into_logical_interest(
            InterestId(7),
            InterestScope::Global,
            InterestLifecycle::Tailing,
        );
        assert!(interest.shape.authors.contains("author_pubkey"));
        assert!(interest.shape.kinds.contains(&30443));
        assert!(interest.shape.kinds.contains(&443));
    }

    #[test]
    fn bridge_threads_limit_into_interest_shape() {
        let deps = ViewDependencies {
            kinds: vec![30443],
            authors: vec!["peer_pk".to_string()],
            limit: Some(4),
            ..Default::default()
        };
        let interest = deps.into_logical_interest(
            InterestId(42),
            InterestScope::Global,
            InterestLifecycle::Tailing,
        );
        assert_eq!(interest.shape.limit, Some(4));
    }

    #[test]
    fn bridge_omits_limit_when_none() {
        let deps = ViewDependencies {
            kinds: vec![1],
            ..Default::default()
        };
        let interest = deps.into_logical_interest(
            InterestId(1),
            InterestScope::Global,
            InterestLifecycle::Tailing,
        );
        assert_eq!(interest.shape.limit, None);
    }
}
