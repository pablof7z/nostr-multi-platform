//! `GroupMembersView` — projection of the latest 39001 + 39002 snapshots.

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS};

use super::shared::{EventAccumulator, EventAccumulatorDelta};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MembersSpec { pub group: GroupId }

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MembersPayload {
    pub admins: Vec<String>,
    pub members: Vec<String>,
}

pub struct GroupMembersView;
impl ViewModule for GroupMembersView {
    const NAMESPACE: &'static str = "nip29.group_members";
    type Spec = MembersSpec;
    type Payload = MembersPayload;
    type Delta = EventAccumulatorDelta;
    type Key = GroupId;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key { spec.group.clone() }
    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS],
            tag_refs: vec![("d".into(), spec.group.local_id.clone())],
            ..Default::default()
        }
    }
    fn open(_c: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (EventAccumulator::default(), MembersPayload { admins: Vec::new(), members: Vec::new() })
    }
    fn on_event_inserted(_c: &ViewContext, s: &mut Self::State, e: &KernelEvent) -> Option<Self::Delta> { s.insert(e) }
    fn on_event_removed(_c: &ViewContext, s: &mut Self::State, id: &EventId) -> Option<Self::Delta> { s.remove(id) }
    fn on_event_replaced(_c: &ViewContext, s: &mut Self::State, old: &EventId, e: &KernelEvent) -> Option<Self::Delta> { s.replace(old, e) }
    fn on_projection_changed(_c: &ViewContext, _s: &mut Self::State, _ch: &ProjectionChange) -> Option<Self::Delta> { None }

    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        let pick_latest = |kind: u32| -> Option<&KernelEvent> {
            state.events.iter().filter(|e| e.kind == kind).max_by_key(|e| e.created_at)
        };
        let collect_p_tags = |e: &KernelEvent| -> Vec<String> {
            e.tags.iter()
                .filter(|t| t.len() >= 2 && t[0] == "p")
                .map(|t| t[1].clone())
                .collect()
        };
        let admins = pick_latest(KIND_GROUP_ADMINS).map(collect_p_tags).unwrap_or_default();
        let members = pick_latest(KIND_GROUP_MEMBERS).map(collect_p_tags).unwrap_or_default();
        MembersPayload { admins, members }
    }
}
