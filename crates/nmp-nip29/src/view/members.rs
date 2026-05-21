//! `GroupMembersView` — projection of the latest 39001 + 39002 snapshots.

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
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
impl GroupMembersView {
    pub const NAMESPACE: &'static str = "nip29.group_members";

    pub fn key(spec: &MembersSpec) -> GroupId { spec.group.clone() }
    pub fn dependencies(spec: &MembersSpec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS],
            tag_refs: vec![("d".into(), spec.group.local_id.clone())],
            ..Default::default()
        }
    }
    pub fn open(_c: &ViewContext, _spec: MembersSpec) -> (EventAccumulator, MembersPayload) {
        (EventAccumulator::default(), MembersPayload { admins: Vec::new(), members: Vec::new() })
    }
    pub fn on_event_inserted(_c: &ViewContext, s: &mut EventAccumulator, e: &KernelEvent) -> Option<EventAccumulatorDelta> { s.insert(e) }
    pub fn on_event_removed(_c: &ViewContext, s: &mut EventAccumulator, id: &EventId) -> Option<EventAccumulatorDelta> { s.remove(id) }
    pub fn on_event_replaced(_c: &ViewContext, s: &mut EventAccumulator, old: &EventId, e: &KernelEvent) -> Option<EventAccumulatorDelta> { s.replace(old, e) }

    pub fn snapshot(_c: &ViewContext, state: &EventAccumulator) -> MembersPayload {
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
