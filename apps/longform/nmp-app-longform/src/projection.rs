//! `LongformProjection` — the read-side of the long-form article reader.
//!
//! A [`nmp_core::KernelEventObserver`] that filters incoming kernel events
//! down to kind:30023 (NIP-23 long-form), extracts the article's title from
//! the `["title", …]` tag, and accumulates a deduped, sorted-by-newest-first
//! list in a host-owned store.
//!
//! Pure consumption — no actions, no relay writes. The matching publish side
//! is intentionally not built (the spike is read-only).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nmp_core::{substrate::KernelEvent, KernelEventObserver};
use serde::{Deserialize, Serialize};

/// NIP-23 long-form content event kind.
pub const KIND_LONGFORM: u32 = 30023;

/// Snapshot key the host-extensible projection registers under. Reachable via
/// `KernelSnapshot::projections["longform.articles"]` on every snapshot tick.
pub const ARTICLES_SNAPSHOT_KEY: &str = "longform.articles";

/// One projected article. Shape matches the spike spec: id + title + author + created_at.
///
/// `id` is the NIP-01 event id (hex). `author` is the event's pubkey (hex).
/// `title` falls back to the empty string when the event omits a `["title", …]`
/// tag — D6: we surface every accepted kind:30023 event, even unconventionally
/// tagged ones, and let the host UI decide how to render a titleless article.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Article {
    pub id: String,
    pub title: String,
    pub author: String,
    pub created_at: u64,
}

/// Host-owned store: id → article. `HashMap` (not `BTreeMap`) because we sort
/// on output by `created_at` desc, not by id. Wrapped in `Arc<Mutex<…>>` so
/// the observer (actor thread) and the FFI getter (caller thread) share one
/// instance — same pattern as `fixture-todo-core::TodoStore`.
pub type ArticleStore = Arc<Mutex<HashMap<String, Article>>>;

/// `LongformProjection` is the spike's [`KernelEventObserver`]: every accepted
/// event the kernel ingests fans to [`Self::on_kernel_event`], we filter to
/// kind:30023, and we upsert into the shared store.
///
/// **Dedup posture**: NIP-23 is a parameterised-replaceable event (kind 30023);
/// the same `(author, "d" tag)` pair re-publishes multiple times and only the
/// newest should win. We approximate that here by keying on the event `id` and
/// keeping the row with the larger `created_at` when an id collision happens.
/// A complete NIP-23 reader would also dedupe by `(author, d_tag)` and prefer
/// the newer revision — that is a known follow-up, recorded in the README under
/// "what was missing from the substrate".
pub struct LongformProjection {
    store: ArticleStore,
}

impl LongformProjection {
    /// Construct a projection that writes into `store`. The host retains a
    /// clone of the same `Arc` and reads from it in the snapshot projection /
    /// FFI getter.
    #[must_use]
    pub fn new(store: ArticleStore) -> Self {
        Self { store }
    }

    /// Project the current store into the snapshot-payload JSON value the host
    /// contributes under [`ARTICLES_SNAPSHOT_KEY`]. Articles are sorted by
    /// `created_at` descending (newest first) — matches the spike spec's
    /// "sorted list of articles" requirement.
    ///
    /// Factored out as a free method so the snapshot-projection closure and
    /// the FFI getter both call it (no duplicated sort/serialize).
    pub fn snapshot_json(&self) -> serde_json::Value {
        let Ok(guard) = self.store.lock() else {
            // D6 — a poisoned store mutex degrades to an empty list rather than
            // panicking inside the snapshot tick.
            return serde_json::json!({ "articles": [] });
        };
        let mut articles: Vec<Article> = guard.values().cloned().collect();
        articles.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        serde_json::json!({ "articles": articles })
    }
}

impl KernelEventObserver for LongformProjection {
    /// Fires on the actor thread for every event the kernel accepts
    /// (`Inserted | Replaced`). D8 — cheap and non-blocking: kind-filter then
    /// one mutex acquire, insert/replace, release.
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_LONGFORM {
            return;
        }
        let article = Article {
            id: event.id.clone(),
            title: extract_title(&event.tags).unwrap_or_default(),
            author: event.author.clone(),
            created_at: event.created_at,
        };
        let Ok(mut guard) = self.store.lock() else {
            return;
        };
        // Newer-wins dedup on the event id: a kernel `Replaced` re-emission
        // arrives here with the same id and a >= created_at, so we just keep
        // the more recent row. A different revision of the same article (same
        // author+d_tag, different id) lands as a separate row — see the
        // docstring on `LongformProjection`.
        match guard.get(&article.id) {
            Some(existing) if existing.created_at >= article.created_at => {}
            _ => {
                guard.insert(article.id.clone(), article);
            }
        }
    }
}

/// Extract the title from a NIP-23 event's tags. Per the spec, the title is
/// in a `["title", "the title"]` tag — first match wins. Returns `None` when
/// the event has no `title` tag or the tag has no second element.
fn extract_title(tags: &[Vec<String>]) -> Option<String> {
    tags.iter()
        .find(|t| t.first().map(String::as_str) == Some("title"))
        .and_then(|t| t.get(1))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str, kind: u32, created_at: u64, title: Option<&str>) -> KernelEvent {
        let mut tags: Vec<Vec<String>> = vec![vec!["d".into(), format!("slug-{id}")]];
        if let Some(t) = title {
            tags.push(vec!["title".into(), t.into()]);
        }
        KernelEvent {
            id: id.into(),
            author: "auth".into(),
            kind,
            created_at,
            tags,
            content: "body".into(),
        }
    }

    #[test]
    fn observer_collects_only_kind_30023() {
        let store: ArticleStore = Arc::new(Mutex::new(HashMap::new()));
        let proj = LongformProjection::new(Arc::clone(&store));

        // A kind:1 note must be ignored.
        proj.on_kernel_event(&ev("note", 1, 100, Some("not an article")));
        // A NIP-23 article must be collected.
        proj.on_kernel_event(&ev("article-a", KIND_LONGFORM, 200, Some("Hello")));

        let guard = store.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard.get("article-a").unwrap().title, "Hello");
    }

    #[test]
    fn observer_extracts_empty_title_when_tag_missing() {
        let store: ArticleStore = Arc::new(Mutex::new(HashMap::new()));
        let proj = LongformProjection::new(Arc::clone(&store));

        proj.on_kernel_event(&ev("untitled", KIND_LONGFORM, 200, None));

        let guard = store.lock().unwrap();
        assert_eq!(guard.get("untitled").unwrap().title, "");
    }

    #[test]
    fn observer_newer_wins_on_id_collision() {
        let store: ArticleStore = Arc::new(Mutex::new(HashMap::new()));
        let proj = LongformProjection::new(Arc::clone(&store));

        proj.on_kernel_event(&ev("a", KIND_LONGFORM, 100, Some("Old")));
        proj.on_kernel_event(&ev("a", KIND_LONGFORM, 200, Some("New")));
        // Older `created_at` must not overwrite the newer row.
        proj.on_kernel_event(&ev("a", KIND_LONGFORM, 50, Some("Ancient")));

        let guard = store.lock().unwrap();
        assert_eq!(guard.get("a").unwrap().title, "New");
        assert_eq!(guard.get("a").unwrap().created_at, 200);
    }

    #[test]
    fn snapshot_sorts_articles_newest_first() {
        let store: ArticleStore = Arc::new(Mutex::new(HashMap::new()));
        let proj = LongformProjection::new(Arc::clone(&store));

        proj.on_kernel_event(&ev("old", KIND_LONGFORM, 100, Some("Old")));
        proj.on_kernel_event(&ev("new", KIND_LONGFORM, 300, Some("New")));
        proj.on_kernel_event(&ev("mid", KIND_LONGFORM, 200, Some("Mid")));

        let snap = proj.snapshot_json();
        let articles = snap.get("articles").and_then(|v| v.as_array()).unwrap();
        assert_eq!(articles.len(), 3);
        assert_eq!(articles[0].get("id").and_then(|v| v.as_str()), Some("new"));
        assert_eq!(articles[1].get("id").and_then(|v| v.as_str()), Some("mid"));
        assert_eq!(articles[2].get("id").and_then(|v| v.as_str()), Some("old"));
    }

    #[test]
    fn snapshot_empty_store_returns_empty_array() {
        let store: ArticleStore = Arc::new(Mutex::new(HashMap::new()));
        let proj = LongformProjection::new(store);

        let snap = proj.snapshot_json();
        let articles = snap.get("articles").and_then(|v| v.as_array()).unwrap();
        assert!(articles.is_empty());
    }
}
