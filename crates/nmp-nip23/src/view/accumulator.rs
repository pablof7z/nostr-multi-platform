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

/// In-memory state for the two `ViewModule` impls. Public so each view can
/// declare it as its `State` associated type; the inner map is intentionally
/// private so external mutation goes through the `insert` / `remove` /
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
