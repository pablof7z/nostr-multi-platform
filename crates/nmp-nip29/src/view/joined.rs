//! `JoinedGroupsView` — multi-host aggregation of "communities I'm in".
//!
//! Per `routing.md` §4.3 Strategy C: the view dependencies fan out across
//! every host_relay in the `JoinedHostsCache`, producing one host-pinned
//! interest per host for the 39001/39002 stream filtered to the user's pubkey.
//! The cache itself lives in `nmp_nip29::cache::JoinedHostsCache`.

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS};

use super::shared::{EventAccumulator, EventAccumulatorDelta};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JoinedSpec {
    pub user_pubkey: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JoinedPayload {
    /// All groups whose 39001/39002 the user's pubkey appears in. The
    /// payload carries the typed `GroupId` so the UI never has to re-derive
    /// the host from the wire shape.
    pub groups: Vec<GroupId>,
}

pub struct JoinedGroupsView;
impl ViewModule for JoinedGroupsView {
    const NAMESPACE: &'static str = "nip29.joined_groups";
    type Spec = JoinedSpec;
    type Payload = JoinedPayload;
    type Delta = EventAccumulatorDelta;
    type Key = String;
    type State = EventAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key { spec.user_pubkey.clone() }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        // The actual fan-out (one pinned LogicalInterest per host) happens via
        // `interest::joined_groups_for_host` driven by `JoinedHostsCache`. The
        // ViewDependencies surface here is the structural shape the compiler
        // sees pre-fanout — `#p: [self_pubkey]` on 39001/39002. The kernel
        // wraps each of these in a `pin_to: Some(host)` per-host interest at
        // dispatch time.
        ViewDependencies {
            kinds: vec![KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS],
            tag_refs: vec![("p".into(), spec.user_pubkey.clone())],
            ..Default::default()
        }
    }
    fn open(_c: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (EventAccumulator::default(), JoinedPayload { groups: Vec::new() })
    }
    fn on_event_inserted(_c: &ViewContext, s: &mut Self::State, e: &KernelEvent) -> Option<Self::Delta> { s.insert(e) }
    fn on_event_removed(_c: &ViewContext, s: &mut Self::State, id: &EventId) -> Option<Self::Delta> { s.remove(id) }
    fn on_event_replaced(_c: &ViewContext, s: &mut Self::State, old: &EventId, e: &KernelEvent) -> Option<Self::Delta> { s.replace(old, e) }
    fn on_projection_changed(_c: &ViewContext, _s: &mut Self::State, _ch: &ProjectionChange) -> Option<Self::Delta> { None }
    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        // We don't know the host_relay_url from inside the projection here
        // (the kernel's provenance lane carries it; M11.5 Step 5 wires that
        // through). For the Step 0 deliverable, the snapshot is a placeholder
        // that gives the count via state.events.len(); proper GroupId
        // collection requires the provenance hookup.
        let _ = state.events.len();
        JoinedPayload { groups: Vec::new() }
    }
}
