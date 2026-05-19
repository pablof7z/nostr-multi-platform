//! `ListView` — every NIP-51 list of a given `(author, kind)`, sorted by
//! `created_at` desc.
//!
//! The `Key` is the composite `(PublicKey, u32)` tuple — a *true* composite
//! dependency key per D8, not a per-event allocation and not an
//! `Option<author>` (a "lists of every kind by every author" query is not a
//! real product query — both axes are always pinned by the opening screen).

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::decode::ListRecord;

use super::accumulator::{ListAccumulator, ListViewDelta};
use super::PublicKey;

/// Spec carries the author **and** the list kind — both required. A
/// list-of-a-kind-for-an-author is the real query (e.g. "alice's follow
/// sets"); the composite is `(author, kind)`.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ListListSpec {
    /// List author (hex pubkey).
    pub author: PublicKey,
    /// One of the six NIP-51 kinds.
    pub kind: u32,
}

/// Best-effort (D1) payload — always a renderable `Vec`, never a loading gate.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ListListPayload {
    /// Lists matching `(author, kind)`, `created_at` desc.
    pub lists: Vec<ListRecord>,
}

/// `ViewModule` projecting every list of a given `(author, kind)`.
pub struct ListView;
impl ViewModule for ListView {
    const NAMESPACE: &'static str = "nmp.nip51.list";
    type Spec = ListListSpec;
    type Payload = ListListPayload;
    type Delta = ListViewDelta;
    /// True composite dependency key — `(author, kind)`.
    type Key = (PublicKey, u32);
    type State = ListAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key {
        (spec.author.clone(), spec.kind)
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![spec.kind],
            authors: vec![spec.author.clone()],
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (ListAccumulator::default(), ListListPayload::default())
    }

    fn on_event_inserted(
        _ctx: &ViewContext,
        state: &mut Self::State,
        event: &KernelEvent,
    ) -> Option<Self::Delta> {
        state.insert(event)
    }

    fn on_event_removed(
        _ctx: &ViewContext,
        state: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        state.remove(id)
    }

    fn on_event_replaced(
        _ctx: &ViewContext,
        state: &mut Self::State,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<Self::Delta> {
        state.replace(old_id, new_event)
    }

    fn on_projection_changed(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _change: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }

    fn snapshot(_ctx: &ViewContext, state: &Self::State) -> Self::Payload {
        ListListPayload {
            lists: state.snapshot_sorted(),
        }
    }
}
