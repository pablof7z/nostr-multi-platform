//! `GroupHomeView` — single-group landing page projection.
//!
//! Surfaces metadata + admin/member counts + recent chat preview + recent
//! discussions preview. Cross-protocol joins (profile hydration) live at the
//! app layer.

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{
    KIND_CHAT_MESSAGE, KIND_DISCUSSION_OR_ARTIFACT, KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS,
    KIND_GROUP_METADATA,
};

use super::shared::{EventAccumulator, EventAccumulatorDelta};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HomeSpec { pub group: GroupId }

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct HomePayload {
    pub metadata_event_count: usize,
    pub admin_event_count: usize,
    pub member_event_count: usize,
    pub chat_preview_count: usize,
    pub discussion_preview_count: usize,
}

pub struct GroupHomeView;
impl GroupHomeView {
    pub const NAMESPACE: &'static str = "nip29.group_home";

    pub fn key(spec: &HomeSpec) -> GroupId { spec.group.clone() }
    pub fn dependencies(spec: &HomeSpec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![
                KIND_GROUP_METADATA, KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS,
                KIND_CHAT_MESSAGE, KIND_DISCUSSION_OR_ARTIFACT,
            ],
            tag_refs: vec![("h".into(), spec.group.local_id.clone())],
            ..Default::default()
        }
    }
    pub fn open(_c: &ViewContext, _spec: HomeSpec) -> (EventAccumulator, HomePayload) {
        (EventAccumulator::default(), HomePayload::default())
    }
    pub fn on_event_inserted(_c: &ViewContext, s: &mut EventAccumulator, e: &KernelEvent) -> Option<EventAccumulatorDelta> { s.insert(e) }
    pub fn on_event_removed(_c: &ViewContext, s: &mut EventAccumulator, id: &EventId) -> Option<EventAccumulatorDelta> { s.remove(id) }
    pub fn on_event_replaced(_c: &ViewContext, s: &mut EventAccumulator, old: &EventId, e: &KernelEvent) -> Option<EventAccumulatorDelta> { s.replace(old, e) }

    pub fn snapshot(_c: &ViewContext, state: &EventAccumulator) -> HomePayload {
        let mut p = HomePayload::default();
        for e in &state.events {
            match e.kind {
                KIND_GROUP_METADATA => p.metadata_event_count += 1,
                KIND_GROUP_ADMINS => p.admin_event_count += 1,
                KIND_GROUP_MEMBERS => p.member_event_count += 1,
                KIND_CHAT_MESSAGE => p.chat_preview_count += 1,
                KIND_DISCUSSION_OR_ARTIFACT => p.discussion_preview_count += 1,
                _ => {}
            }
        }
        p
    }
}
