use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::hash::Hash;

use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
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
    /// Host-relay this view's interest must be pinned to (NIP-29 single-group
    /// views, Marmot group-relay views). `None` means the standard outbox/inbox
    /// routing applies. The kernel does not yet act on this field — it is
    /// declared here so host-pinned views express their relay affinity in the
    /// data model rather than via discarded side-channel helpers.
    pub relay_pin: Option<String>,
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
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle,
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
pub struct ViewContext {
    pub now_ms: u64,
}

pub trait ViewModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;

    type Spec: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Payload: Clone + Serialize + Send + 'static;
    type Delta: Clone + Serialize + Send + 'static;
    type Key: Hash + Eq + Clone + Serialize + Send + 'static;
    type State: Send + 'static;

    fn key(spec: &Self::Spec) -> Self::Key;
    /// Returns the events this view needs. Callers should convert the result via
    /// [`ViewDependencies::into_logical_interest`] to obtain a [`LogicalInterest`]
    /// for `NmpApp::push_interest`.
    fn dependencies(spec: &Self::Spec) -> ViewDependencies;
    fn open(ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload);

    fn on_event_inserted(
        ctx: &ViewContext,
        state: &mut Self::State,
        event: &KernelEvent,
    ) -> Option<Self::Delta>;

    fn on_event_removed(
        ctx: &ViewContext,
        state: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta>;

    fn on_event_replaced(
        ctx: &ViewContext,
        state: &mut Self::State,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<Self::Delta>;

    fn on_projection_changed(
        ctx: &ViewContext,
        state: &mut Self::State,
        change: &ProjectionChange,
    ) -> Option<Self::Delta>;

    fn on_tick(_ctx: &ViewContext, _state: &mut Self::State) -> Option<Self::Delta> {
        None
    }

    fn snapshot(ctx: &ViewContext, state: &Self::State) -> Self::Payload;
}

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
        assert_eq!(interest.shape.relay_pin.as_deref(), Some("wss://group.relay/"));
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
}
