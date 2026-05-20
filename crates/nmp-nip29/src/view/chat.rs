//! `GroupChatView`, `GroupDiscussionsView`, `GroupArtifactsView` — single-group
//! event-list projections, all host-pinned via the same `relay_pin` mechanism.

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{
    KIND_CHAT_MESSAGE, KIND_DISCUSSION_OR_ARTIFACT, KIND_HIGHLIGHT, KIND_REPOST,
};

use super::shared::{EventAccumulator, EventAccumulatorDelta};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ChatSpec {
    pub group: GroupId,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ChatPayload {
    pub events: Vec<KernelEvent>,
}

pub struct GroupChatView;
impl ViewModule for GroupChatView {
    const NAMESPACE: &'static str = "nip29.group_chat";
    type Spec = ChatSpec;
    type Payload = ChatPayload;
    type Delta = EventAccumulatorDelta;
    type Key = GroupId;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key { spec.group.clone() }
    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_CHAT_MESSAGE],
            tag_refs: vec![("h".into(), spec.group.local_id.clone())],
            relay_pin: Some(spec.group.host_relay_url.clone()),
            ..Default::default()
        }
    }
    fn open(_ctx: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (EventAccumulator::default(), ChatPayload { events: Vec::new() })
    }
    fn on_event_inserted(_c: &ViewContext, s: &mut Self::State, e: &KernelEvent) -> Option<Self::Delta> { s.insert(e) }
    fn on_event_removed(_c: &ViewContext, s: &mut Self::State, id: &EventId) -> Option<Self::Delta> { s.remove(id) }
    fn on_event_replaced(_c: &ViewContext, s: &mut Self::State, old: &EventId, e: &KernelEvent) -> Option<Self::Delta> { s.replace(old, e) }
    fn on_projection_changed(_c: &ViewContext, _s: &mut Self::State, _ch: &ProjectionChange) -> Option<Self::Delta> { None }
    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        ChatPayload { events: state.events.clone() }
    }
}

// ── Discussions (kind:11 with t=discussion) ──────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiscussionsSpec { pub group: GroupId }
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiscussionsPayload { pub events: Vec<KernelEvent> }

pub struct GroupDiscussionsView;
impl ViewModule for GroupDiscussionsView {
    const NAMESPACE: &'static str = "nip29.group_discussions";
    type Spec = DiscussionsSpec;
    type Payload = DiscussionsPayload;
    type Delta = EventAccumulatorDelta;
    type Key = GroupId;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key { spec.group.clone() }
    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_DISCUSSION_OR_ARTIFACT],
            tag_refs: vec![
                ("h".into(), spec.group.local_id.clone()),
                ("t".into(), "discussion".into()),
            ],
            relay_pin: Some(spec.group.host_relay_url.clone()),
            ..Default::default()
        }
    }
    fn open(_c: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (EventAccumulator::default(), DiscussionsPayload { events: Vec::new() })
    }
    fn on_event_inserted(_c: &ViewContext, s: &mut Self::State, e: &KernelEvent) -> Option<Self::Delta> {
        // Only accept events carrying t=discussion (artifact shares share kind:11
        // but live in GroupArtifactsView).
        let has_marker = e.tags.iter().any(|t| t.len() >= 2 && t[0] == "t" && t[1] == "discussion");
        if !has_marker { return None; }
        s.insert(e)
    }
    fn on_event_removed(_c: &ViewContext, s: &mut Self::State, id: &EventId) -> Option<Self::Delta> { s.remove(id) }
    fn on_event_replaced(_c: &ViewContext, s: &mut Self::State, old: &EventId, e: &KernelEvent) -> Option<Self::Delta> { s.replace(old, e) }
    fn on_projection_changed(_c: &ViewContext, _s: &mut Self::State, _ch: &ProjectionChange) -> Option<Self::Delta> { None }
    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        DiscussionsPayload { events: state.events.clone() }
    }
}

// ── Room Library lanes: artifacts + reposts + h-tagged highlights ────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ArtifactsSpec { pub group: GroupId }
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ArtifactsPayload { pub events: Vec<KernelEvent> }

pub struct GroupArtifactsView;
impl ViewModule for GroupArtifactsView {
    const NAMESPACE: &'static str = "nip29.group_artifacts";
    type Spec = ArtifactsSpec;
    type Payload = ArtifactsPayload;
    type Delta = EventAccumulatorDelta;
    type Key = GroupId;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key { spec.group.clone() }
    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        // kind:11 (without t=discussion, filtered post-ingest) + kind:16 +
        // kind:9802 with the group's h tag — see GroupArtifacts ViewModule
        // entry in nip29-crate.md §3.2.
        ViewDependencies {
            kinds: vec![KIND_DISCUSSION_OR_ARTIFACT, KIND_REPOST, KIND_HIGHLIGHT],
            tag_refs: vec![("h".into(), spec.group.local_id.clone())],
            relay_pin: Some(spec.group.host_relay_url.clone()),
            ..Default::default()
        }
    }
    fn open(_c: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (EventAccumulator::default(), ArtifactsPayload { events: Vec::new() })
    }
    fn on_event_inserted(_c: &ViewContext, s: &mut Self::State, e: &KernelEvent) -> Option<Self::Delta> {
        // kind:11 events with t=discussion belong in GroupDiscussionsView, not here.
        if e.kind == KIND_DISCUSSION_OR_ARTIFACT {
            let is_discussion = e.tags.iter().any(|t| t.len() >= 2 && t[0] == "t" && t[1] == "discussion");
            if is_discussion { return None; }
        }
        s.insert(e)
    }
    fn on_event_removed(_c: &ViewContext, s: &mut Self::State, id: &EventId) -> Option<Self::Delta> { s.remove(id) }
    fn on_event_replaced(_c: &ViewContext, s: &mut Self::State, old: &EventId, e: &KernelEvent) -> Option<Self::Delta> { s.replace(old, e) }
    fn on_projection_changed(_c: &ViewContext, _s: &mut Self::State, _ch: &ProjectionChange) -> Option<Self::Delta> { None }
    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        ArtifactsPayload { events: state.events.clone() }
    }
}
