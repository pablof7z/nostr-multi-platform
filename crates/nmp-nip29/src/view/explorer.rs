//! `GroupExplorerView` — discoverable groups on a single host relay.
//!
//! Lists all 39000 events without the `hidden` marker, host-pinned to one
//! relay. Used by the "Room Explorer" surface in `Features/Communities/`.

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::group_id::RelayUrl;
use crate::kinds::KIND_GROUP_METADATA;

use super::shared::{EventAccumulator, EventAccumulatorDelta};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ExplorerSpec {
    pub host_relay_url: RelayUrl,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ExplorerPayload {
    pub group_count: usize,
}

pub struct GroupExplorerView;
impl ViewModule for GroupExplorerView {
    const NAMESPACE: &'static str = "nip29.group_explorer";
    type Spec = ExplorerSpec;
    type Payload = ExplorerPayload;
    type Delta = EventAccumulatorDelta;
    type Key = RelayUrl;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key { spec.host_relay_url.clone() }
    fn dependencies(_spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_GROUP_METADATA],
            ..Default::default()
        }
    }
    fn open(_c: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (EventAccumulator::default(), ExplorerPayload { group_count: 0 })
    }
    fn on_event_inserted(_c: &ViewContext, s: &mut Self::State, e: &KernelEvent) -> Option<Self::Delta> {
        // Filter out `hidden` groups per nip29-crate.md §7 deferral list note.
        let hidden = e.tags.iter().any(|t| !t.is_empty() && t[0] == "hidden");
        if hidden { return None; }
        s.insert(e)
    }
    fn on_event_removed(_c: &ViewContext, s: &mut Self::State, id: &EventId) -> Option<Self::Delta> { s.remove(id) }
    fn on_event_replaced(_c: &ViewContext, s: &mut Self::State, old: &EventId, e: &KernelEvent) -> Option<Self::Delta> { s.replace(old, e) }
    fn on_projection_changed(_c: &ViewContext, _s: &mut Self::State, _ch: &ProjectionChange) -> Option<Self::Delta> { None }
    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        ExplorerPayload { group_count: state.events.len() }
    }
}
