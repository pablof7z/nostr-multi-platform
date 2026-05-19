//! `ArticleListView` — list articles, optionally filtered by author, sorted
//! by `published_at` desc.

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::decode::ArticleRecord;
use crate::kinds::KIND_LONG_FORM_ARTICLE;

use super::accumulator::{ArticleAccumulator, ArticleViewDelta};
use super::PublicKey;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub struct ArticleListSpec {
    /// Optional author filter. `None` → list every article visible to the kernel.
    pub author: Option<PublicKey>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ArticleListPayload {
    pub articles: Vec<ArticleRecord>,
}

pub struct ArticleListView;
impl ViewModule for ArticleListView {
    const NAMESPACE: &'static str = "nmp.nip23.article_list";
    type Spec = ArticleListSpec;
    type Payload = ArticleListPayload;
    type Delta = ArticleViewDelta;
    type Key = Option<PublicKey>;
    type State = ArticleAccumulator;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.author.clone()
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_LONG_FORM_ARTICLE],
            authors: spec.author.iter().cloned().collect(),
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        (ArticleAccumulator::default(), ArticleListPayload::default())
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
        ArticleListPayload {
            articles: state.snapshot_sorted(),
        }
    }
}
