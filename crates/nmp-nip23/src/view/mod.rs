//! `ViewModule` impls for NIP-23 articles.
//!
//! Two views per the task brief:
//! - [`ArticleListView`] — list articles, optionally filtered by author,
//!   sorted by `published_at` desc.
//! - [`ArticleDetailView`] — single article by `(author, kind, d_tag)`
//!   coordinate (`NaddrCoord` — the structured form of an `naddr1…`).
//!
//! Each view's `State` is the article-aware [`ArticleAccumulator`] which
//! dedupes by `(author, d_tag)` per NIP-33 replaceability. Decoding happens
//! once at insert time (D8 hot-path discipline) and the snapshot is the
//! sorted `Vec<ArticleRecord>` views consume.

mod accumulator;
mod detail;
mod list;

pub use accumulator::{ArticleAccumulator, ArticleViewDelta};
pub use detail::{ArticleDetailPayload, ArticleDetailSpec, ArticleDetailView};
pub use list::{ArticleListPayload, ArticleListSpec, ArticleListView};

/// Hex-encoded pubkey alias matching `nmp_core::planner::Pubkey` — surfaced
/// here so the view specs don't force callers to import the planner module
/// for one type alias.
pub type PublicKey = String;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::KIND_LONG_FORM_ARTICLE;
    use nmp_core::planner::NaddrCoord;
    use nmp_core::substrate::{KernelEvent, ViewContext, ViewModule};

    fn ke(id: &str, kind: u32, author: &str, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: author.into(),
            kind,
            created_at,
            tags,
            content: format!("body-{id}"),
        }
    }

    fn article_event(id: &str, author: &str, created_at: u64, d_tag: &str, published_at: Option<u64>) -> KernelEvent {
        let mut tags = vec![vec!["d".into(), d_tag.into()]];
        if let Some(ts) = published_at {
            tags.push(vec!["published_at".into(), ts.to_string()]);
        }
        ke(id, KIND_LONG_FORM_ARTICLE, author, created_at, tags)
    }

    #[test]
    fn list_view_dependencies_filter_by_author_when_set() {
        let deps = ArticleListView::dependencies(&ArticleListSpec {
            author: Some("alice".into()),
        });
        assert_eq!(deps.kinds, vec![KIND_LONG_FORM_ARTICLE]);
        assert_eq!(deps.authors, vec!["alice".to_string()]);
    }

    #[test]
    fn list_view_dependencies_unfiltered_when_author_none() {
        let deps = ArticleListView::dependencies(&ArticleListSpec { author: None });
        assert!(deps.authors.is_empty());
    }

    #[test]
    fn list_view_snapshot_sorts_by_published_desc() {
        let (mut state, _) = ArticleListView::open(
            &ViewContext::default(),
            ArticleListSpec { author: None },
        );
        ArticleListView::on_event_inserted(
            &ViewContext::default(),
            &mut state,
            &article_event("e1", "alice", 100, "a", Some(100)),
        );
        ArticleListView::on_event_inserted(
            &ViewContext::default(),
            &mut state,
            &article_event("e2", "bob", 50, "b", Some(200)),
        );
        let payload = ArticleListView::snapshot(&ViewContext::default(), &state);
        let order: Vec<_> = payload.articles.iter().map(|r| r.d_tag.as_str()).collect();
        assert_eq!(order, vec!["b", "a"]);
    }

    #[test]
    fn list_view_replaces_via_nip33_dedup_keeps_newer() {
        let (mut state, _) = ArticleListView::open(
            &ViewContext::default(),
            ArticleListSpec { author: None },
        );
        ArticleListView::on_event_inserted(
            &ViewContext::default(),
            &mut state,
            &article_event("e1", "alice", 100, "intro", None),
        );
        ArticleListView::on_event_inserted(
            &ViewContext::default(),
            &mut state,
            &article_event("e2", "alice", 200, "intro", None),
        );
        let payload = ArticleListView::snapshot(&ViewContext::default(), &state);
        assert_eq!(payload.articles.len(), 1);
        assert_eq!(payload.articles[0].event_id, "e2");
    }

    #[test]
    fn list_view_drops_non_30023_events() {
        let (mut state, _) = ArticleListView::open(
            &ViewContext::default(),
            ArticleListSpec { author: None },
        );
        let bad = ke("e1", 1, "alice", 100, vec![vec!["d".into(), "x".into()]]);
        let delta = ArticleListView::on_event_inserted(&ViewContext::default(), &mut state, &bad);
        assert!(delta.is_none());
        let payload = ArticleListView::snapshot(&ViewContext::default(), &state);
        assert!(payload.articles.is_empty());
    }

    #[test]
    fn list_view_remove_drops_the_article() {
        let (mut state, _) = ArticleListView::open(
            &ViewContext::default(),
            ArticleListSpec { author: None },
        );
        ArticleListView::on_event_inserted(
            &ViewContext::default(),
            &mut state,
            &article_event("e1", "alice", 100, "intro", None),
        );
        let delta = ArticleListView::on_event_removed(
            &ViewContext::default(),
            &mut state,
            &"e1".to_string(),
        );
        assert!(matches!(delta, Some(ArticleViewDelta::Removed(_))));
        let payload = ArticleListView::snapshot(&ViewContext::default(), &state);
        assert!(payload.articles.is_empty());
    }

    #[test]
    fn detail_view_dependencies_declare_full_triple() {
        let coord = NaddrCoord {
            pubkey: "alice".into(),
            kind: KIND_LONG_FORM_ARTICLE,
            d_tag: "intro".into(),
        };
        let deps = ArticleDetailView::dependencies(&ArticleDetailSpec { coord: coord.clone() });
        assert_eq!(deps.kinds, vec![KIND_LONG_FORM_ARTICLE]);
        assert_eq!(deps.authors, vec!["alice".to_string()]);
        assert_eq!(deps.tag_refs, vec![("d".to_string(), "intro".to_string())]);
    }

    #[test]
    fn detail_view_payload_is_coord_placeholder_when_no_event_seen() {
        // D1: the payload is always renderable. Before the authoritative
        // event arrives, `article` is a deterministic placeholder synthesised
        // from the coord and `source == "placeholder"` — never `Option::None`.
        let coord = NaddrCoord {
            pubkey: "alice".into(),
            kind: KIND_LONG_FORM_ARTICLE,
            d_tag: "intro".into(),
        };
        let (state, opened) = ArticleDetailView::open(
            &ViewContext::default(),
            ArticleDetailSpec { coord: coord.clone() },
        );
        assert_eq!(opened.source, "placeholder");
        assert_eq!(opened.article.author, coord.pubkey);
        assert_eq!(opened.article.d_tag, coord.d_tag);
        assert!(opened.article.event_id.is_empty());

        let payload = ArticleDetailView::snapshot(&ViewContext::default(), &state);
        assert_eq!(payload.source, "placeholder");
        assert_eq!(payload.article.author, "alice");
        assert_eq!(payload.article.d_tag, "intro");
    }
}
