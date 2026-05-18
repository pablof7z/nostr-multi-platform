//! `ArticleDetailView` — single article resolved by `(author, kind, d_tag)`
//! coordinate (the structured form of an `naddr1…` bech32).

use nmp_core::planner::NaddrCoord;
use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::decode::ArticleRecord;

use super::accumulator::{ArticleAccumulator, ArticleViewDelta};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub struct ArticleDetailSpec {
    /// Naddr coordinate — `(author pubkey, kind, d_tag)`. The bech32-encoded
    /// `naddr1…` form is decoded into `NaddrCoord` by `nmp-nip19` (future
    /// crate); this view consumes the structured form per the "no
    /// string-typed coordinates" rule in `kind-wrappers.md` §9 #4.
    pub coord: NaddrCoord,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ArticleDetailPayload {
    pub article: Option<ArticleRecord>,
}

pub struct ArticleDetailView;
impl ViewModule for ArticleDetailView {
    const NAMESPACE: &'static str = "nmp.nip23.article_detail";
    type Spec = ArticleDetailSpec;
    type Payload = ArticleDetailPayload;
    type Delta = ArticleViewDelta;
    type Key = NaddrCoord;
    type State = ArticleAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.coord.clone()
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        // Address-pointer hydration: declare the full triple. The compiler's
        // Rule 8 (address-pointer) and the store's `idx_kind_dtag` then do the
        // routing + lookup work — this view stays a pure consumer.
        ViewDependencies {
            kinds: vec![spec.coord.kind],
            authors: vec![spec.coord.pubkey.clone()],
            tag_refs: vec![("d".into(), spec.coord.d_tag.clone())],
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (ArticleAccumulator::default(), ArticleDetailPayload::default())
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
        // The kernel REQ filtered on the full triple, so any record in the
        // accumulator is a valid candidate. `snapshot_sorted()[0]` is the
        // current (NIP-33 newest) replaceable instance.
        let article = state.snapshot_sorted().into_iter().next();
        ArticleDetailPayload { article }
    }
}
