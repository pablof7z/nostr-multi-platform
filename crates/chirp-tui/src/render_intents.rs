use std::collections::BTreeSet;

use crate::timeline::TimelineRow;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RenderIntent {
    AuthorProfile { pubkey: String },
    NoteRelations { event_id: String },
}

#[derive(Default)]
pub struct RenderIntentTracker {
    active: BTreeSet<RenderIntent>,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct RenderIntentDiff {
    pub added: Vec<RenderIntent>,
    pub removed: Vec<RenderIntent>,
}

impl RenderIntentTracker {
    pub fn sync_rows(&mut self, rows: &[TimelineRow]) -> RenderIntentDiff {
        let next = intents_for_rows(rows);
        let added = next.difference(&self.active).cloned().collect();
        let removed = self.active.difference(&next).cloned().collect();
        self.active = next;
        RenderIntentDiff { added, removed }
    }
}

fn intents_for_rows(rows: &[TimelineRow]) -> BTreeSet<RenderIntent> {
    let mut intents = BTreeSet::new();
    for row in rows {
        if !row.author_pubkey.is_empty() {
            intents.insert(RenderIntent::AuthorProfile {
                pubkey: row.author_pubkey.clone(),
            });
        }
        if !row.id.is_empty() {
            intents.insert(RenderIntent::NoteRelations {
                event_id: row.id.clone(),
            });
        }
    }
    intents
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, pubkey: &str) -> TimelineRow {
        TimelineRow {
            id: id.to_string(),
            author: pubkey.to_string(),
            author_pubkey: pubkey.to_string(),
            content: String::new(),
            created_at: 1,
            depth: 0,
            has_gap: false,
            relation_counts: Default::default(),
        }
    }

    #[test]
    fn first_render_claims_unique_authors_and_notes() {
        let mut tracker = RenderIntentTracker::default();
        let diff = tracker.sync_rows(&[row("n1", "alice"), row("n2", "alice")]);

        assert_eq!(diff.removed, Vec::new());
        assert_eq!(diff.added.len(), 3);
        assert!(diff.added.contains(&RenderIntent::AuthorProfile {
            pubkey: "alice".to_string()
        }));
    }

    #[test]
    fn scrolling_adds_and_removes_only_changed_intents() {
        let mut tracker = RenderIntentTracker::default();
        tracker.sync_rows(&[row("n1", "alice"), row("n2", "bob")]);

        let diff = tracker.sync_rows(&[row("n2", "bob"), row("n3", "carol")]);

        assert!(diff.removed.contains(&RenderIntent::AuthorProfile {
            pubkey: "alice".to_string()
        }));
        assert!(diff.added.contains(&RenderIntent::AuthorProfile {
            pubkey: "carol".to_string()
        }));
    }

    #[test]
    fn empty_view_releases_prior_intents() {
        let mut tracker = RenderIntentTracker::default();
        tracker.sync_rows(&[row("n1", "alice")]);

        let diff = tracker.sync_rows(&[]);

        assert!(diff.added.is_empty());
        assert_eq!(diff.removed.len(), 2);
    }
}
