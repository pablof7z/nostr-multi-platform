//! `ArticleDetailView` — single article resolved by `(author, kind, d_tag)`
//! coordinate (the structured form of an `naddr1…` bech32).

use nmp_core::planner::NaddrCoord;
use nmp_core::substrate::{
    EventId, KernelEvent, Placeholder, ProjectionChange, ViewContext, ViewDependencies,
    ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::decode::ArticleRecord;

use super::accumulator::{ArticleAccumulator, ArticleViewDelta};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub struct ArticleDetailSpec {
    /// Naddr coordinate — `(author pubkey, kind, d_tag)`. The bech32-encoded
    /// `naddr1…` form is decoded into `NaddrCoord` by `nmp_core::nip19`; this
    /// view consumes the structured form per the "no string-typed
    /// coordinates" rule in `kind-wrappers.md` §9 #4.
    pub coord: NaddrCoord,
}

/// D1 contract (best-effort rendering, ADR-0017): the detail payload always
/// carries a renderable `article`. Before the authoritative kind:30023 event
/// arrives, `article` is a deterministic placeholder synthesised from the
/// requested coord; `source` is the discriminator the UI branches on, exactly
/// as `TimelineItem.author_avatar_source` does for profile pictures. The field
/// is never `Option` and never crosses FFI as nullable.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArticleDetailPayload {
    pub article: Placeholder<ArticleRecord>,
    /// `"placeholder"` until the authoritative event is decoded, then
    /// `"decoded"`.
    pub source: String,
}

/// Coord-scoped detail state. The shared [`ArticleAccumulator`] is keyed only
/// on `(author, d_tag)` and is also used by `ArticleListView`; on its own it
/// would admit a misrouted same-`d` event from a *different* author and
/// surface the wrong article (codex review finding #2). We therefore store the
/// spec's `NaddrCoord` alongside the accumulator and reject any event whose
/// triple does not match it, so the view can never observe an off-coord
/// record.
pub struct DetailState {
    coord: NaddrCoord,
    inner: ArticleAccumulator,
}

impl DetailState {
    fn event_matches_coord(&self, event: &KernelEvent) -> bool {
        if event.author != self.coord.pubkey || event.kind != self.coord.kind {
            return false;
        }
        let d_tag = event
            .tags
            .iter()
            .find(|t| t.first().map(String::as_str) == Some("d"))
            .and_then(|t| t.get(1))
            .map(String::as_str);
        d_tag == Some(self.coord.d_tag.as_str())
    }
}

/// Synthesise the D1 placeholder article for a coord that has not yet resolved
/// to an authoritative event. Deterministic in the coord so SwiftUI diffing
/// sees no spurious churn before the real event lands.
fn placeholder_article(coord: &NaddrCoord) -> ArticleRecord {
    ArticleRecord {
        event_id: String::new(),
        author: coord.pubkey.clone(),
        d_tag: coord.d_tag.clone(),
        title: None,
        image: None,
        summary: None,
        published_at: None,
        created_at: 0,
        content: String::new(),
        tags: Vec::new(),
    }
}

pub struct ArticleDetailView;
impl ViewModule for ArticleDetailView {
    const NAMESPACE: &'static str = "nmp.nip23.article_detail";
    type Spec = ArticleDetailSpec;
    type Payload = ArticleDetailPayload;
    type Delta = ArticleViewDelta;
    type Key = NaddrCoord;
    type State = DetailState;

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

    fn open(_ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let payload = ArticleDetailPayload {
            article: Placeholder(placeholder_article(&spec.coord)),
            source: "placeholder".into(),
        };
        let state = DetailState {
            coord: spec.coord,
            inner: ArticleAccumulator::default(),
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
            // The replacement is off-coord; only honour the removal so a stale
            // off-coord id can never linger, but never admit the new event.
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
        // Every record in `inner` already passed the coord filter on insert,
        // so `snapshot_sorted()[0]` is the current (NIP-33 newest) instance of
        // exactly the requested article. Absent an authoritative event we fall
        // back to the deterministic D1 placeholder.
        match state.inner.snapshot_sorted().into_iter().next() {
            Some(article) => ArticleDetailPayload {
                article: Placeholder(article),
                source: "decoded".into(),
            },
            None => ArticleDetailPayload {
                article: Placeholder(placeholder_article(&state.coord)),
                source: "placeholder".into(),
            },
        }
    }
}
