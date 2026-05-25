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
        // Demand-driven kind:0 fetch for every NIP-21 profile mention
        // appearing inside this row's content. The `BTreeSet` dedupes the
        // case where the same pubkey is both the row author AND a content
        // mention (one claim, not two), and the case where the same pubkey
        // is mentioned in multiple rows on screen. Empty `mention_pubkeys`
        // (no content tree, no mentions) is a zero-cost no-op.
        for pubkey in &row.mention_pubkeys {
            if !pubkey.is_empty() {
                intents.insert(RenderIntent::AuthorProfile {
                    pubkey: pubkey.clone(),
                });
            }
        }
    }
    intents
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, pubkey: &str) -> TimelineRow {
        row_with_mentions(id, pubkey, Vec::new())
    }

    fn row_with_mentions(id: &str, pubkey: &str, mention_pubkeys: Vec<String>) -> TimelineRow {
        TimelineRow {
            id: id.to_string(),
            author: pubkey.to_string(),
            author_pubkey: pubkey.to_string(),
            content: String::new(),
            media: Vec::new(),
            created_at: 1,
            depth: 0,
            has_gap: false,
            relation_counts: Default::default(),
            mention_pubkeys,
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

    #[test]
    fn content_mentions_emit_author_profile_intents() {
        let mut tracker = RenderIntentTracker::default();
        let mention = "1".repeat(64);
        let diff = tracker.sync_rows(&[row_with_mentions(
            "n1",
            "alice",
            vec![mention.clone()],
        )]);

        // (1) author + note + mention = 3 distinct intents on first render.
        assert_eq!(diff.removed, Vec::new());
        assert_eq!(diff.added.len(), 3);
        assert!(diff.added.contains(&RenderIntent::AuthorProfile {
            pubkey: "alice".to_string()
        }));
        assert!(diff.added.contains(&RenderIntent::AuthorProfile {
            pubkey: mention.clone(),
        }));
        assert!(diff.added.contains(&RenderIntent::NoteRelations {
            event_id: "n1".to_string()
        }));
    }

    #[test]
    fn mention_pubkey_overlapping_with_author_dedupes_to_single_claim() {
        let mut tracker = RenderIntentTracker::default();
        let alice = "a".repeat(64);
        // Row authored by alice, content mentions alice — must produce ONE
        // AuthorProfile intent for alice, not two.
        let diff = tracker.sync_rows(&[row_with_mentions(
            "n1",
            &alice,
            vec![alice.clone()],
        )]);
        let alice_intents: Vec<&RenderIntent> = diff
            .added
            .iter()
            .filter(|intent| {
                matches!(intent, RenderIntent::AuthorProfile { pubkey } if pubkey == &alice)
            })
            .collect();
        assert_eq!(
            alice_intents.len(),
            1,
            "author and mention claims for the same pubkey must dedupe"
        );
    }

    #[test]
    fn dropping_a_mention_releases_only_that_pubkey() {
        let mut tracker = RenderIntentTracker::default();
        let m1 = "1".repeat(64);
        let m2 = "2".repeat(64);
        tracker.sync_rows(&[row_with_mentions(
            "n1",
            "alice",
            vec![m1.clone(), m2.clone()],
        )]);

        // Second snapshot drops `m2`, keeps `m1`.
        let diff = tracker.sync_rows(&[row_with_mentions(
            "n1",
            "alice",
            vec![m1.clone()],
        )]);

        assert!(diff.removed.contains(&RenderIntent::AuthorProfile {
            pubkey: m2,
        }));
        assert!(!diff.removed.contains(&RenderIntent::AuthorProfile {
            pubkey: m1,
        }));
        assert!(diff.added.is_empty());
    }
}
