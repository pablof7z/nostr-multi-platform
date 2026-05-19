//! `ListDetailView` — a single set resolved by `(author, kind, d_tag)`
//! coordinate (the structured form of an `naddr1…` bech32).

use nmp_core::planner::NaddrCoord;
use nmp_core::substrate::{
    EventId, KernelEvent, Placeholder, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::decode::{ListItems, ListKind, ListRecord};

use super::accumulator::{ListAccumulator, ListViewDelta};

/// Spec — the naddr coordinate `(author pubkey, kind, d_tag)`.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ListDetailSpec {
    /// Structured naddr coordinate. The bech32 `naddr1…` form is decoded into
    /// `NaddrCoord` by `nmp_core::nip19`; this view consumes the structured
    /// form per the "no string-typed coordinates" rule (`kind-wrappers.md`
    /// §9 #4).
    pub coord: NaddrCoord,
}

/// D1 contract: the detail payload always carries a renderable `list`. Before
/// the authoritative event arrives it is a deterministic placeholder
/// synthesised from the coord; `source` is the discriminator the UI branches
/// on. Never `Option`, never nullable across FFI.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ListDetailPayload {
    /// The resolved (or placeholder) list.
    pub list: Placeholder<ListRecord>,
    /// `"placeholder"` until the authoritative event is decoded, then
    /// `"decoded"`.
    pub source: String,
}

/// Coord-scoped detail state. The shared [`ListAccumulator`] keys on
/// `(author, kind, d_tag)` and is also used by `ListView`; on its own it would
/// admit a misrouted same-`d` event from a *different author* (or, since this
/// crate spans six kinds, a same-author same-`d` event of a *different kind*)
/// and surface the wrong list (codex review finding #2). We store the coord
/// alongside the accumulator and reject any event whose full triple does not
/// match it.
pub struct DetailState {
    coord: NaddrCoord,
    inner: ListAccumulator,
}

impl DetailState {
    fn event_matches_coord(&self, event: &KernelEvent) -> bool {
        // Kind is load-bearing here: a mute list and a relay list by one
        // author can share `d_tag == ""`, so the coord filter MUST check kind.
        if event.author != self.coord.pubkey || event.kind != self.coord.kind {
            return false;
        }
        let d_tag = event
            .tags
            .iter()
            .find(|t| t.first().map(String::as_str) == Some("d"))
            .and_then(|t| t.get(1))
            .map(String::as_str)
            .unwrap_or("");
        d_tag == self.coord.d_tag.as_str()
    }
}

/// Synthesise the D1 placeholder list for an unresolved coord. Deterministic in
/// the coord so SwiftUI diffing sees no spurious churn before the real event
/// lands. `list_kind` falls back to [`ListKind::Mute`] when the coord's kind is
/// outside the six (the coord filter rejects such events anyway, so this is a
/// never-surfaced default that keeps the type total).
fn placeholder_list(coord: &NaddrCoord) -> ListRecord {
    ListRecord {
        event_id: String::new(),
        author: coord.pubkey.clone(),
        list_kind: ListKind::from_kind(coord.kind).unwrap_or(ListKind::Mute),
        d_tag: coord.d_tag.clone(),
        title: None,
        description: None,
        image: None,
        items: ListItems::default(),
        encrypted_payload: String::new(),
        created_at: 0,
        tags: Vec::new(),
    }
}

/// `ViewModule` resolving a single list/set by its naddr coordinate.
pub struct ListDetailView;
impl ViewModule for ListDetailView {
    const NAMESPACE: &'static str = "nmp.nip51.list_detail";
    type Spec = ListDetailSpec;
    type Payload = ListDetailPayload;
    type Delta = ListViewDelta;
    type Key = NaddrCoord;
    type State = DetailState;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.coord.clone()
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![spec.coord.kind],
            authors: vec![spec.coord.pubkey.clone()],
            tag_refs: vec![("d".into(), spec.coord.d_tag.clone())],
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let payload = ListDetailPayload {
            list: Placeholder(placeholder_list(&spec.coord)),
            source: "placeholder".into(),
        };
        let state = DetailState {
            coord: spec.coord,
            inner: ListAccumulator::default(),
        };
        (state, payload)
    }

    fn on_event_inserted(
        _ctx: &ViewContext,
        state: &mut Self::State,
        event: &KernelEvent,
    ) -> Option<Self::Delta> {
        if !state.event_matches_coord(event) {
            return None;
        }
        state.inner.insert(event)
    }

    fn on_event_removed(
        _ctx: &ViewContext,
        state: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        state.inner.remove(id)
    }

    fn on_event_replaced(
        _ctx: &ViewContext,
        state: &mut Self::State,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<Self::Delta> {
        if !state.event_matches_coord(new_event) {
            // Off-coord replacement: honour the removal so a stale id can't
            // linger, but never admit the new event.
            return state.inner.remove(old_id);
        }
        state.inner.replace(old_id, new_event)
    }

    fn on_projection_changed(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _change: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }

    fn snapshot(_ctx: &ViewContext, state: &Self::State) -> Self::Payload {
        // Every record in `inner` passed the coord filter on insert, so the
        // newest is exactly the requested list. Absent an authoritative event
        // we fall back to the deterministic D1 placeholder.
        match state.inner.snapshot_sorted().into_iter().next() {
            Some(list) => ListDetailPayload {
                list: Placeholder(list),
                source: "decoded".into(),
            },
            None => ListDetailPayload {
                list: Placeholder(placeholder_list(&state.coord)),
                source: "placeholder".into(),
            },
        }
    }
}
