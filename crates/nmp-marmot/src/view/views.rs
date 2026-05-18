//! `GroupList`, `GroupMessages`, `MemberList` ViewModule impls per
//! `docs/plan/marmot-mls.md` §Step 1 + mdk-api.md §6.
//!
//! `GroupMessages` is relay-pinned to the group relay (kind:445) via
//! `InterestShape::relay_pin` (ADR-0012). `GroupList` / `MemberList` project
//! off service-materialised state (the actual member set + decrypted history
//! come from MDK, not the raw wire), so their dependency surface is the
//! relay-pinned kind:445 stream that triggers re-projection ticks; the
//! decrypted payload is filled by the service/actor layer (same scope-cut
//! as nmp-nip29's Step 0 views).

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use super::shared::{EventAccumulator, EventAccumulatorDelta};
use crate::interest::{KIND_GROUP_MESSAGE, KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY};

// ─── GroupList ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GroupListSpec {
    /// The local identity pubkey (hex) whose joined groups to list.
    pub self_pubkey: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GroupListEntry {
    pub group_id_hex: String,
    pub group_relay_url: String,
    pub name: String,
    pub unread: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GroupListPayload {
    pub groups: Vec<GroupListEntry>,
}

/// List of joined + pending Marmot groups with unread counts. The authoritative
/// group set comes from `MDK::get_groups()` via [`crate::service`]; this view's
/// snapshot is filled by the service/actor layer (the wire is ciphertext).
pub struct GroupListView;
impl ViewModule for GroupListView {
    const NAMESPACE: &'static str = "marmot.group_list";
    type Spec = GroupListSpec;
    type Payload = GroupListPayload;
    type Delta = EventAccumulatorDelta;
    type Key = String;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.self_pubkey.clone()
    }
    fn dependencies(_spec: &Self::Spec) -> ViewDependencies {
        // KeyPackage stream (own publications, standard outbox — no pin) is the
        // structural trigger surface; group membership itself is MDK state.
        ViewDependencies {
            kinds: vec![KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY],
            ..Default::default()
        }
    }
    fn open(_c: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (
            EventAccumulator::default(),
            GroupListPayload { groups: Vec::new() },
        )
    }
    fn on_event_inserted(
        _c: &ViewContext,
        s: &mut Self::State,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.insert(e)
    }
    fn on_event_removed(
        _c: &ViewContext,
        s: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        s.remove(id)
    }
    fn on_event_replaced(
        _c: &ViewContext,
        s: &mut Self::State,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.replace(old, e)
    }
    fn on_projection_changed(
        _c: &ViewContext,
        _s: &mut Self::State,
        _ch: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }
    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        // Authoritative list is MDK-side; service fills this. Count surfaces
        // the structural accumulator size as a liveness signal.
        let _ = state.events.len();
        GroupListPayload { groups: Vec::new() }
    }
}

// ─── GroupMessages ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GroupMessagesSpec {
    /// Hex MLS group id.
    pub group_id_hex: String,
    /// The group relay all kind:445 events are pinned to.
    pub group_relay_url: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GroupMessageEntry {
    pub message_id: String,
    pub sender_pubkey: String,
    pub created_at: u64,
    pub content: String,
    pub epoch: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GroupMessagesPayload {
    pub messages: Vec<GroupMessageEntry>,
}

/// Paginated decrypted message stream for a group; live-updates on new epoch.
/// Relay-pinned to the group relay (kind:445). Decrypted content is filled by
/// the service after `MDK::process_message`.
pub struct GroupMessagesView;
impl ViewModule for GroupMessagesView {
    const NAMESPACE: &'static str = "marmot.group_messages";
    type Spec = GroupMessagesSpec;
    type Payload = GroupMessagesPayload;
    type Delta = EventAccumulatorDelta;
    type Key = String;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.group_id_hex.clone()
    }
    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        // kind:445 group-event stream. The kernel wraps this in a
        // relay_pin: Some(group_relay) interest at dispatch time per the
        // interest helper (ADR-0012 third lane). The structural surface here
        // declares the kind; the pin is carried via interest::group_messages.
        let _ = &spec.group_relay_url;
        ViewDependencies {
            kinds: vec![KIND_GROUP_MESSAGE],
            ..Default::default()
        }
    }
    fn open(_c: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (
            EventAccumulator::default(),
            GroupMessagesPayload {
                messages: Vec::new(),
            },
        )
    }
    fn on_event_inserted(
        _c: &ViewContext,
        s: &mut Self::State,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.insert(e)
    }
    fn on_event_removed(
        _c: &ViewContext,
        s: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        s.remove(id)
    }
    fn on_event_replaced(
        _c: &ViewContext,
        s: &mut Self::State,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.replace(old, e)
    }
    fn on_projection_changed(
        _c: &ViewContext,
        _s: &mut Self::State,
        _ch: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }
    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        let _ = state.events.len();
        GroupMessagesPayload {
            messages: Vec::new(),
        }
    }
}

// ─── MemberList ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MemberListSpec {
    pub group_id_hex: String,
    pub group_relay_url: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MemberEntry {
    pub pubkey: String,
    /// MLS leaf index within the ratchet tree.
    pub leaf_index: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MemberListPayload {
    pub members: Vec<MemberEntry>,
}

/// Current group member list with MLS leaf indices. Authoritative set comes
/// from `MDK::get_members()` + `MDK::group_leaf_map()` via [`crate::service`].
pub struct MemberListView;
impl ViewModule for MemberListView {
    const NAMESPACE: &'static str = "marmot.member_list";
    type Spec = MemberListSpec;
    type Payload = MemberListPayload;
    type Delta = EventAccumulatorDelta;
    type Key = String;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.group_id_hex.clone()
    }
    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        // Member changes arrive as kind:445 commits; the kernel relay-pins
        // this to the group relay (interest::group_messages).
        let _ = &spec.group_relay_url;
        ViewDependencies {
            kinds: vec![KIND_GROUP_MESSAGE],
            ..Default::default()
        }
    }
    fn open(_c: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (
            EventAccumulator::default(),
            MemberListPayload {
                members: Vec::new(),
            },
        )
    }
    fn on_event_inserted(
        _c: &ViewContext,
        s: &mut Self::State,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.insert(e)
    }
    fn on_event_removed(
        _c: &ViewContext,
        s: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        s.remove(id)
    }
    fn on_event_replaced(
        _c: &ViewContext,
        s: &mut Self::State,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.replace(old, e)
    }
    fn on_projection_changed(
        _c: &ViewContext,
        _s: &mut Self::State,
        _ch: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }
    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        let _ = state.events.len();
        MemberListPayload {
            members: Vec::new(),
        }
    }
}
