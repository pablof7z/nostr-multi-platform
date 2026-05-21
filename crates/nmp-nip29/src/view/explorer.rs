//! `GroupExplorerView` — discoverable groups on a single host relay.
//!
//! Lists all 39000 events without the `hidden` marker, host-pinned to one
//! relay. Used by the "Room Explorer" surface in `Features/Communities/`.

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
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
impl GroupExplorerView {
    pub const NAMESPACE: &'static str = "nip29.group_explorer";

    pub fn key(spec: &ExplorerSpec) -> RelayUrl { spec.host_relay_url.clone() }
    pub fn dependencies(_spec: &ExplorerSpec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_GROUP_METADATA],
            ..Default::default()
        }
    }
    pub fn open(_c: &ViewContext, _spec: ExplorerSpec) -> (EventAccumulator, ExplorerPayload) {
        (EventAccumulator::default(), ExplorerPayload { group_count: 0 })
    }
    pub fn on_event_inserted(_c: &ViewContext, s: &mut EventAccumulator, e: &KernelEvent) -> Option<EventAccumulatorDelta> {
        // Filter out `hidden` groups per nip29-crate.md §7 deferral list note.
        let hidden = e.tags.iter().any(|t| !t.is_empty() && t[0] == "hidden");
        if hidden { return None; }
        s.insert(e)
    }
    pub fn on_event_removed(_c: &ViewContext, s: &mut EventAccumulator, id: &EventId) -> Option<EventAccumulatorDelta> { s.remove(id) }
    pub fn on_event_replaced(_c: &ViewContext, s: &mut EventAccumulator, old: &EventId, e: &KernelEvent) -> Option<EventAccumulatorDelta> { s.replace(old, e) }
    pub fn snapshot(_c: &ViewContext, state: &EventAccumulator) -> ExplorerPayload {
        ExplorerPayload { group_count: state.events.len() }
    }
}
