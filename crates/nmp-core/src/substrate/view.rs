use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::hash::Hash;

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
