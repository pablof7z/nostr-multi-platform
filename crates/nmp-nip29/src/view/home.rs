//! `GroupHomeView` — single-group landing page projection.
//!
//! Surfaces metadata + admin/member counts + recent chat preview + recent
//! discussions preview. Cross-protocol joins (profile hydration) live at the
//! app layer (`nmp-highlighter-core`).

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
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
impl ViewModule for GroupHomeView {
    const NAMESPACE: &'static str = "nip29.group_home";
    type Spec = HomeSpec;
    type Payload = HomePayload;
    type Delta = EventAccumulatorDelta;
    type Key = GroupId;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key { spec.group.clone() }
    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![
                KIND_GROUP_METADATA, KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS,
                KIND_CHAT_MESSAGE, KIND_DISCUSSION_OR_ARTIFACT,
            ],
            tag_refs: vec![("h".into(), spec.group.local_id.clone())],
            ..Default::default()
        }
    }
    fn open(_c: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (EventAccumulator::default(), HomePayload::default())
    }
    fn on_event_inserted(_c: &ViewContext, s: &mut Self::State, e: &KernelEvent) -> Option<Self::Delta> { s.insert(e) }
    fn on_event_removed(_c: &ViewContext, s: &mut Self::State, id: &EventId) -> Option<Self::Delta> { s.remove(id) }
    fn on_event_replaced(_c: &ViewContext, s: &mut Self::State, old: &EventId, e: &KernelEvent) -> Option<Self::Delta> { s.replace(old, e) }
    fn on_projection_changed(_c: &ViewContext, _s: &mut Self::State, _ch: &ProjectionChange) -> Option<Self::Delta> { None }

    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
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

