//! Shared `ArticleAccumulator` used by both `ArticleListView` and
//! `ArticleDetailView`.
//!
//! Article-aware accumulator (vs nip29's generic `EventAccumulator`): holds
//! the latest article per `(author, d_tag)` (NIP-33 replaceability) keyed on
//! that tuple; produces sorted `Vec<ArticleRecord>` snapshots views consume.

use std::collections::BTreeMap;

use nmp_core::substrate::{EventId, KernelEvent};
use serde::{Deserialize, Serialize};

use crate::decode::{try_from_kernel_event, ArticleRecord};

/// In-memory state for the two NIP-23 views. Public so each view can use it
/// as its state type; the inner map is intentionally private so external
/// mutation goes through the `insert` / `remove` /
/// `replace` API that enforces NIP-33 replaceability.
#[derive(Default)]
pub struct ArticleAccumulator {
    records: BTreeMap<(String, String), ArticleRecord>,
}

impl ArticleAccumulator {
    fn key_of(record: &ArticleRecord) -> (String, String) {
        (record.author.clone(), record.d_tag.clone())
    }

    /// Insert (or replace) the article decoded from `event`. Returns the delta
    /// — `None` if the event is not a valid article or is staler than the
    /// existing record for the same `(author, d_tag)` pair.
    pub fn insert(&mut self, event: &KernelEvent) -> Option<ArticleViewDelta> {
        let record = try_from_kernel_event(event)?;
        let key = Self::key_of(&record);
        if let Some(existing) = self.records.get(&key) {
            if existing.created_at >= record.created_at {
                return None;
            }
        }
        self.records.insert(key, record);
        Some(ArticleViewDelta::Updated(event.id.clone()))
    }

    /// Remove the article whose `event_id` matches `id`.
    pub fn remove(&mut self, id: &EventId) -> Option<ArticleViewDelta> {
        let removed_key = self
            .records
            .iter()
            .find(|(_, r)| r.event_id == *id)
            .map(|(k, _)| k.clone())?;
        self.records.remove(&removed_key);
        Some(ArticleViewDelta::Removed(id.clone()))
    }

    /// Replace the article keyed on `old_id` with the new event's decoded form.
    pub fn replace(
        &mut self,
        old_id: &EventId,
        event: &KernelEvent,
    ) -> Option<ArticleViewDelta> {
        self.remove(old_id);
        self.insert(event)
    }

    /// All articles sorted by `published_at` desc (falls back to `created_at`
    /// when `published_at` is absent).
    pub fn snapshot_sorted(&self) -> Vec<ArticleRecord> {
        let mut out: Vec<ArticleRecord> = self.records.values().cloned().collect();
        out.sort_by(|a, b| {
            let a_ts = a.published_at.unwrap_or(a.created_at);
            let b_ts = b.published_at.unwrap_or(b.created_at);
            b_ts.cmp(&a_ts)
        });
        out
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ArticleViewDelta {
    Updated(EventId),
    Removed(EventId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::KIND_LONG_FORM_ARTICLE;

    /// Build a kind:30023 `KernelEvent`. `published_at` is emitted as a tag only
    /// when `Some`, mirroring the optionality the decoder treats it with.
    fn article_event(
        id: &str,
        author: &str,
        created_at: u64,
        d_tag: &str,
        published_at: Option<u64>,
    ) -> KernelEvent {
        let mut tags = vec![vec!["d".to_string(), d_tag.to_string()]];
        if let Some(ts) = published_at {
            tags.push(vec!["published_at".to_string(), ts.to_string()]);
        }
        KernelEvent {
            id: id.to_string(),
            author: author.to_string(),
            kind: KIND_LONG_FORM_ARTICLE,
            created_at,
            tags,
            content: format!("body-{id}"),
        }
    }

    #[test]
    fn insert_rejects_non_article_kind() {
        let mut acc = ArticleAccumulator::default();
        let not_article = KernelEvent {
            id: "e1".into(),
            author: "alice".into(),
            kind: 1,
            created_at: 100,
            tags: vec![vec!["d".into(), "x".into()]],
            content: String::new(),
        };
        assert!(acc.insert(&not_article).is_none());
        assert!(acc.snapshot_sorted().is_empty());
    }

    #[test]
    fn insert_rejects_article_without_d_tag() {
        let mut acc = ArticleAccumulator::default();
        let no_d = KernelEvent {
            id: "e1".into(),
            author: "alice".into(),
            kind: KIND_LONG_FORM_ARTICLE,
            created_at: 100,
            tags: vec![vec!["title".into(), "Untitled".into()]],
            content: String::new(),
        };
        assert!(acc.insert(&no_d).is_none());
        assert!(acc.snapshot_sorted().is_empty());
    }

    #[test]
    fn insert_with_equal_created_at_keeps_the_incumbent() {
        // NIP-33 staleness gate is `existing.created_at >= record.created_at`,
        // so an equal-`created_at` redelivery is treated as stale and the
        // incumbent record (and its event id) is retained — never clobbered by
        // a same-timestamp duplicate from a second relay.
        let mut acc = ArticleAccumulator::default();
        acc.insert(&article_event("e1", "alice", 100, "intro", None))
            .expect("first insert lands");
        let delta = acc.insert(&article_event("e2", "alice", 100, "intro", None));
        assert!(
            delta.is_none(),
            "an equal-created_at redelivery is stale, not an update"
        );
        let snap = acc.snapshot_sorted();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].event_id, "e1", "the incumbent is retained on a tie");
    }

    #[test]
    fn snapshot_sort_falls_back_to_created_at_when_published_at_absent() {
        // When `published_at` is absent the sort key is `created_at`. Two
        // articles, neither carrying `published_at`, must order by `created_at`
        // descending.
        let mut acc = ArticleAccumulator::default();
        acc.insert(&article_event("e-old", "alice", 100, "a", None));
        acc.insert(&article_event("e-new", "bob", 300, "b", None));
        let order: Vec<String> = acc
            .snapshot_sorted()
            .into_iter()
            .map(|r| r.event_id)
            .collect();
        assert_eq!(order, vec!["e-new".to_string(), "e-old".to_string()]);
    }

    #[test]
    fn snapshot_sort_prefers_published_at_over_created_at() {
        // `published_at` (original publish time) outranks the event header's
        // `created_at` (which a republish bumps). An article republished
        // *later* but originally published *earlier* must sort below a freshly
        // published one.
        let mut acc = ArticleAccumulator::default();
        // Republished recently (created_at 900) but originally old (pub 100).
        acc.insert(&article_event("e-republished", "alice", 900, "old-essay", Some(100)));
        // Published once, recently (pub 800).
        acc.insert(&article_event("e-fresh", "bob", 200, "new-essay", Some(800)));
        let snap = acc.snapshot_sorted();
        assert_eq!(
            snap[0].event_id, "e-fresh",
            "published_at, not created_at, drives display order"
        );
    }

    #[test]
    fn replace_with_unknown_old_id_still_inserts_the_new_event() {
        // `replace` is `remove(old_id)` then `insert(event)`. When `old_id` was
        // never stored the remove is a no-op and the new event must still land
        // — replace must not become conditional on a successful removal.
        let mut acc = ArticleAccumulator::default();
        let delta = acc.replace(
            &"never-seen".to_string(),
            &article_event("e-new", "alice", 100, "intro", None),
        );
        assert!(matches!(delta, Some(ArticleViewDelta::Updated(_))));
        let snap = acc.snapshot_sorted();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].event_id, "e-new");
    }

    #[test]
    fn replace_evicts_the_old_id_even_across_different_keys() {
        // `replace` removes by event id, not by (author, d_tag). If the old and
        // new events are different articles entirely, the old one is gone and
        // only the new one remains.
        let mut acc = ArticleAccumulator::default();
        acc.insert(&article_event("e-old", "alice", 100, "first", None));
        let delta = acc.replace(
            &"e-old".to_string(),
            &article_event("e-new", "alice", 200, "second", None),
        );
        assert!(matches!(delta, Some(ArticleViewDelta::Updated(_))));
        let d_tags: Vec<String> = acc
            .snapshot_sorted()
            .into_iter()
            .map(|r| r.d_tag)
            .collect();
        assert_eq!(
            d_tags,
            vec!["second".to_string()],
            "old article evicted, new one present"
        );
    }

    #[test]
    fn remove_unknown_id_is_a_noop() {
        let mut acc = ArticleAccumulator::default();
        acc.insert(&article_event("e1", "alice", 100, "intro", None));
        assert!(acc.remove(&"not-here".to_string()).is_none());
        assert_eq!(acc.snapshot_sorted().len(), 1);
    }
}
