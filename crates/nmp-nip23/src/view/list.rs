//! `ArticleListView` — list articles, optionally filtered by author, sorted
//! by `published_at` desc.

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
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
impl ArticleListView {
    pub const NAMESPACE: &'static str = "nmp.nip23.article_list";

    pub fn key(spec: &ArticleListSpec) -> Option<PublicKey> {
        spec.author.clone()
    }

    pub fn dependencies(spec: &ArticleListSpec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_LONG_FORM_ARTICLE],
            authors: spec.author.iter().cloned().collect(),
            ..Default::default()
        }
    }

    pub fn open(
        _ctx: &ViewContext,
        _spec: ArticleListSpec,
    ) -> (ArticleAccumulator, ArticleListPayload) {
        (ArticleAccumulator::default(), ArticleListPayload::default())
    }

    pub fn on_event_inserted(
        _ctx: &ViewContext,
        state: &mut ArticleAccumulator,
        event: &KernelEvent,
    ) -> Option<ArticleViewDelta> {
        state.insert(event)
    }

    pub fn on_event_removed(
        _ctx: &ViewContext,
        state: &mut ArticleAccumulator,
        id: &EventId,
    ) -> Option<ArticleViewDelta> {
        state.remove(id)
    }

    pub fn on_event_replaced(
        _ctx: &ViewContext,
        state: &mut ArticleAccumulator,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<ArticleViewDelta> {
        state.replace(old_id, new_event)
    }

    pub fn snapshot(_ctx: &ViewContext, state: &ArticleAccumulator) -> ArticleListPayload {
        ArticleListPayload {
            articles: state.snapshot_sorted(),
        }
    }
}
