//! `RecentGroupEvents` — bounded 50-entry per-group LRU (by `created_at`) of
//! recent event ids, used to populate outbound `["previous", ...]` tags
//! (`moderation.md` §2.3).

use std::collections::{BTreeMap, VecDeque};

use crate::group_id::GroupId;

/// First 8 hex characters of an event id (per `moderation.md` §2 quoted text).
pub type EventIdPrefix = String;

/// Truncate a hex event id to its first 8 chars for `previous`-tag emission.
pub fn previous_tag_prefix(event_id: &str) -> EventIdPrefix {
    event_id.chars().take(8).collect()
}

#[derive(Clone, Debug, Default)]
pub struct RecentGroupEvents {
    /// Per-group LRU. Cap = 50 per `moderation.md` §2.3.
    per_group: BTreeMap<GroupId, VecDeque<RecentEntry>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecentEntry {
    pub event_id: String,
    pub created_at: u64,
}

impl RecentGroupEvents {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, group: &GroupId, event_id: &str, created_at: u64) {
        let entries = self.per_group.entry(group.clone()).or_default();
        if entries.iter().any(|e| e.event_id == event_id) {
            return;
        }
        entries.push_back(RecentEntry {
            event_id: event_id.to_string(),
            created_at,
        });
        while entries.len() > 50 {
            entries.pop_front();
        }
    }

    pub fn previous_tags_for(&self, group: &GroupId, n: usize) -> Vec<EventIdPrefix> {
        let Some(entries) = self.per_group.get(group) else {
            return Vec::new();
        };
        let mut sorted: Vec<&RecentEntry> = entries.iter().collect();
        sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        sorted
            .iter()
            .take(n)
            .map(|e| previous_tag_prefix(&e.event_id))
            .collect()
    }

    pub fn len_for(&self, group: &GroupId) -> usize {
        self.per_group.get(group).map(|d| d.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> GroupId {
        GroupId::new("wss://h.example.com", "g1")
    }

    #[test]
    fn previous_tags_pick_newest_first() {
        let mut rge = RecentGroupEvents::new();
        let g = group();
        rge.record(&g, "aaaaaaaabbbb", 100);
        rge.record(&g, "ccccccccdddd", 200);
        rge.record(&g, "eeeeeeeeffff", 150);
        let tags = rge.previous_tags_for(&g, 2);
        assert_eq!(tags, vec!["cccccccc", "eeeeeeee"]);
    }

    #[test]
    fn recent_cache_bounded_at_50() {
        let mut rge = RecentGroupEvents::new();
        let g = group();
        for i in 0..60u64 {
            rge.record(&g, &format!("{i:064x}"), i);
        }
        assert_eq!(rge.len_for(&g), 50);
    }
}
