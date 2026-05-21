//! `GroupChatView`, `GroupDiscussionsView`, `GroupArtifactsView` — single-group
//! event-list projections, all host-pinned via the same `relay_pin` mechanism.

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
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
impl GroupChatView {
    pub const NAMESPACE: &'static str = "nip29.group_chat";

    pub fn key(spec: &ChatSpec) -> GroupId { spec.group.clone() }
    pub fn dependencies(spec: &ChatSpec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_CHAT_MESSAGE],
            tag_refs: vec![("h".into(), spec.group.local_id.clone())],
            relay_pin: Some(spec.group.host_relay_url.clone()),
            ..Default::default()
        }
    }
    pub fn open(_ctx: &ViewContext, _spec: ChatSpec) -> (EventAccumulator, ChatPayload) {
        (EventAccumulator::default(), ChatPayload { events: Vec::new() })
    }
    pub fn on_event_inserted(_c: &ViewContext, s: &mut EventAccumulator, e: &KernelEvent) -> Option<EventAccumulatorDelta> { s.insert(e) }
    pub fn on_event_removed(_c: &ViewContext, s: &mut EventAccumulator, id: &EventId) -> Option<EventAccumulatorDelta> { s.remove(id) }
    pub fn on_event_replaced(_c: &ViewContext, s: &mut EventAccumulator, old: &EventId, e: &KernelEvent) -> Option<EventAccumulatorDelta> { s.replace(old, e) }
    pub fn snapshot(_c: &ViewContext, state: &EventAccumulator) -> ChatPayload {
        ChatPayload { events: state.events.clone() }
    }
}

// ── Discussions (kind:11 with t=discussion) ──────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiscussionsSpec { pub group: GroupId }
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiscussionsPayload { pub events: Vec<KernelEvent> }

pub struct GroupDiscussionsView;
impl GroupDiscussionsView {
    pub const NAMESPACE: &'static str = "nip29.group_discussions";

    pub fn key(spec: &DiscussionsSpec) -> GroupId { spec.group.clone() }
    pub fn dependencies(spec: &DiscussionsSpec) -> ViewDependencies {
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
    pub fn open(_c: &ViewContext, _spec: DiscussionsSpec) -> (EventAccumulator, DiscussionsPayload) {
        (EventAccumulator::default(), DiscussionsPayload { events: Vec::new() })
    }
    pub fn on_event_inserted(_c: &ViewContext, s: &mut EventAccumulator, e: &KernelEvent) -> Option<EventAccumulatorDelta> {
        // Only accept events carrying t=discussion (artifact shares share kind:11
        // but live in GroupArtifactsView).
        let has_marker = e.tags.iter().any(|t| t.len() >= 2 && t[0] == "t" && t[1] == "discussion");
        if !has_marker { return None; }
        s.insert(e)
    }
    pub fn on_event_removed(_c: &ViewContext, s: &mut EventAccumulator, id: &EventId) -> Option<EventAccumulatorDelta> { s.remove(id) }
    pub fn on_event_replaced(_c: &ViewContext, s: &mut EventAccumulator, old: &EventId, e: &KernelEvent) -> Option<EventAccumulatorDelta> { s.replace(old, e) }
    pub fn snapshot(_c: &ViewContext, state: &EventAccumulator) -> DiscussionsPayload {
        DiscussionsPayload { events: state.events.clone() }
    }
}

// ── Room Library lanes: artifacts + reposts + h-tagged highlights ────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ArtifactsSpec { pub group: GroupId }
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ArtifactsPayload { pub events: Vec<KernelEvent> }

pub struct GroupArtifactsView;
impl GroupArtifactsView {
    pub const NAMESPACE: &'static str = "nip29.group_artifacts";

    pub fn key(spec: &ArtifactsSpec) -> GroupId { spec.group.clone() }
    pub fn dependencies(spec: &ArtifactsSpec) -> ViewDependencies {
        // kind:11 (without t=discussion, filtered post-ingest) + kind:16 +
        // kind:9802 with the group's h tag — see the GroupArtifacts view
        // entry in nip29-crate.md §3.2.
        ViewDependencies {
            kinds: vec![KIND_DISCUSSION_OR_ARTIFACT, KIND_REPOST, KIND_HIGHLIGHT],
            tag_refs: vec![("h".into(), spec.group.local_id.clone())],
            relay_pin: Some(spec.group.host_relay_url.clone()),
            ..Default::default()
        }
    }
    pub fn open(_c: &ViewContext, _spec: ArtifactsSpec) -> (EventAccumulator, ArtifactsPayload) {
        (EventAccumulator::default(), ArtifactsPayload { events: Vec::new() })
    }
    pub fn on_event_inserted(_c: &ViewContext, s: &mut EventAccumulator, e: &KernelEvent) -> Option<EventAccumulatorDelta> {
        // kind:11 events with t=discussion belong in GroupDiscussionsView, not here.
        if e.kind == KIND_DISCUSSION_OR_ARTIFACT {
            let is_discussion = e.tags.iter().any(|t| t.len() >= 2 && t[0] == "t" && t[1] == "discussion");
            if is_discussion { return None; }
        }
        s.insert(e)
    }
    pub fn on_event_removed(_c: &ViewContext, s: &mut EventAccumulator, id: &EventId) -> Option<EventAccumulatorDelta> { s.remove(id) }
    pub fn on_event_replaced(_c: &ViewContext, s: &mut EventAccumulator, old: &EventId, e: &KernelEvent) -> Option<EventAccumulatorDelta> { s.replace(old, e) }
    pub fn snapshot(_c: &ViewContext, state: &EventAccumulator) -> ArtifactsPayload {
        ArtifactsPayload { events: state.events.clone() }
    }
}
