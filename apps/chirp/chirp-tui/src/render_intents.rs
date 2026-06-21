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
    use crate::app::{AppState, Pane};
    use crate::features::FeatureTab;
    use crate::ui::nostr_user::profile_wire::ProfileWire;

    fn row(id: &str, pubkey: &str) -> TimelineRow {
        row_with_mentions(id, pubkey, Vec::new())
    }

    fn row_with_mentions(id: &str, pubkey: &str, mention_pubkeys: Vec<String>) -> TimelineRow {
        TimelineRow {
            id: id.to_string(),
            author_pubkey: pubkey.to_string(),
            author_profile: profile(pubkey),
            content: String::new(),
            created_at: 1,
            depth: 0,
            has_gap: false,
            thread_attribution: Vec::new(),
            relation_counts: Default::default(),
            relay_provenance: Vec::new(),
            content_tree: None,
            content_render: Default::default(),
            mention_pubkeys,
            repost: None,
        }
    }

    fn profile(pubkey: &str) -> ProfileWire {
        ProfileWire {
            pubkey: pubkey.to_string(),
            display_name: Some(pubkey.to_string()),
            about: None,
            picture_url: None,
            nip05: None,
            npub: pubkey.to_string(),
            npub_short: pubkey.to_string(),
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
        let diff = tracker.sync_rows(&[row_with_mentions("n1", "alice", vec![mention.clone()])]);

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
        let diff = tracker.sync_rows(&[row_with_mentions("n1", &alice, vec![alice.clone()])]);
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
        let diff = tracker.sync_rows(&[row_with_mentions("n1", "alice", vec![m1.clone()])]);

        assert!(diff
            .removed
            .contains(&RenderIntent::AuthorProfile { pubkey: m2 }));
        assert!(!diff
            .removed
            .contains(&RenderIntent::AuthorProfile { pubkey: m1 }));
        assert!(diff.added.is_empty());
    }

    #[test]
    fn active_dynamic_profile_rows_drive_render_intents() {
        let mut tracker = RenderIntentTracker::default();
        let home_author = "a".repeat(64);
        let profile_author = "b".repeat(64);
        let state = AppState {
            tab: FeatureTab::Home,
            focused: Pane::Profile,
            profile_pubkey: profile_author.clone(),
            rows: vec![row("home-note", &home_author)],
            profile_rows: vec![row("profile-note", &profile_author)],
            ..Default::default()
        };

        let diff = tracker.sync_rows(state.render_intent_rows());

        assert!(diff.added.contains(&RenderIntent::AuthorProfile {
            pubkey: profile_author.clone()
        }));
        assert!(diff.added.contains(&RenderIntent::NoteRelations {
            event_id: "profile-note".to_string()
        }));
        assert!(!diff.added.contains(&RenderIntent::AuthorProfile {
            pubkey: home_author
        }));
        assert!(!diff.added.contains(&RenderIntent::NoteRelations {
            event_id: "home-note".to_string()
        }));
    }

    #[test]
    fn switching_to_dynamic_thread_releases_home_intents_and_claims_thread_rows() {
        let mut tracker = RenderIntentTracker::default();
        let home_author = "c".repeat(64);
        let thread_author = "d".repeat(64);
        tracker.sync_rows(&[row("home-note", &home_author)]);
        let state = AppState {
            tab: FeatureTab::Home,
            focused: Pane::Detail,
            thread_event_id: "thread-root".to_string(),
            rows: vec![row("home-note", &home_author)],
            thread_rows: vec![row("thread-note", &thread_author)],
            ..Default::default()
        };

        let diff = tracker.sync_rows(state.render_intent_rows());

        assert!(diff.removed.contains(&RenderIntent::AuthorProfile {
            pubkey: home_author
        }));
        assert!(diff.removed.contains(&RenderIntent::NoteRelations {
            event_id: "home-note".to_string()
        }));
        assert!(diff.added.contains(&RenderIntent::AuthorProfile {
            pubkey: thread_author
        }));
        assert!(diff.added.contains(&RenderIntent::NoteRelations {
            event_id: "thread-note".to_string()
        }));
    }
}
