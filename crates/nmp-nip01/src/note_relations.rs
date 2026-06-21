use std::collections::HashMap;
use std::sync::Arc;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use serde::{Deserialize, Serialize};

use crate::decode::try_from_kernel_event;

/// Cap for the reply index. At most this many individual reply-event → parent
/// mappings are tracked; older entries are evicted when the cap is exceeded,
/// with corresponding count decrements to keep `reply_counts` consistent.
const REPLY_INDEX_CAP: usize = MAX_PROJECTION_MESSAGES * 4;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteRelationCounts {
    pub replies: RelationCount,
    pub reactions: RelationCount,
    pub reposts: RelationCount,
    pub zaps: RelationCount,
    /// NIP-22 kind:1111 comments rooted at this event/address/external id.
    /// Distinct from `replies` (kind:1 NIP-10 threaded notes) — a node can have
    /// both. Counted against the comment's UPPERCASE root scope target.
    pub comments: RelationCount,
}

impl NoteRelationCounts {
    #[must_use]
    pub fn for_note(_event_id: &str, counts: TargetRelationCounts) -> Self {
        Self {
            replies: RelationCount::known(counts.replies),
            reactions: RelationCount::known(counts.reactions),
            reposts: RelationCount::known(counts.reposts),
            zaps: RelationCount::known(counts.zaps),
            comments: RelationCount::known(counts.comments),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RelationCount {
    Known { count: u64 },
    Loading { interest: RelationCountInterest },
}

impl RelationCount {
    #[must_use]
    pub fn known(count: u64) -> Self {
        Self::Known { count }
    }

    #[must_use]
    pub fn loading(interest: RelationCountInterest) -> Self {
        Self::Loading { interest }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RelationCountInterest {
    pub namespace: String,
    pub target_event_id: String,
    pub tag: String,
}

impl RelationCountInterest {
    #[must_use]
    pub fn reactions(event_id: &str) -> Self {
        Self {
            namespace: "nmp.reactions.summary".to_string(),
            target_event_id: event_id.to_string(),
            tag: "e".to_string(),
        }
    }

    #[must_use]
    pub fn reposts(event_id: &str) -> Self {
        Self {
            namespace: "nmp.reactions.reposts".to_string(),
            target_event_id: event_id.to_string(),
            tag: "e".to_string(),
        }
    }

    #[must_use]
    pub fn zaps(event_id: &str) -> Self {
        Self {
            namespace: "nmp.nip57.zaps".to_string(),
            target_event_id: event_id.to_string(),
            tag: "e".to_string(),
        }
    }

    /// Interest in NIP-22 kind:1111 comments rooted at `root_tag_value`. The
    /// uppercase root tag is `E` for an event root (the common card case);
    /// `A`/`I` roots use the same namespace with their own scope tag.
    #[must_use]
    pub fn comments(root_tag_value: &str) -> Self {
        Self {
            namespace: "nmp.nip22.comments".to_string(),
            target_event_id: root_tag_value.to_string(),
            tag: "E".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TargetRelationCounts {
    pub replies: u64,
    pub reactions: u64,
    pub reposts: u64,
    pub zaps: u64,
    pub comments: u64,
}

/// The kind of social relation an event expresses toward a target note.
///
/// `Reply` is native to NIP-01 (a kind:1 NIP-10 threaded reply). The other
/// variants are cross-protocol and are produced by an injected
/// [`NoteRelationClassifier`] (see `nmp-relations`), so this base crate carries
/// no dependency on NIP-18 / NIP-22 / NIP-25 / NIP-57.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelationKind {
    Reply,
    Reaction,
    Repost,
    Zap,
    Comment,
}

/// A single classified relation: the target note it counts against and the
/// kind of relation it expresses. Produced by [`NoteRelationClassifier`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassifiedRelation {
    /// The id (or UPPERCASE root scope value, for comments) of the note this
    /// relation is counted against.
    pub target: String,
    /// Which relation bucket this event increments.
    pub kind: RelationKind,
}

/// Cross-protocol seam: classify a kernel event into the [`ClassifiedRelation`]
/// it expresses toward a target note, or `None` if it is not a relation event.
///
/// NIP-01 handles its own kind:1 replies natively; this trait is the injection
/// point for the cross-protocol relation sources (reactions, reposts, zaps,
/// comments) so that aggregation lives in `nmp-relations`, not in the base note
/// crate (#1728). Inject a concrete classifier via [`NoteRelationIndex::new`]
/// or [`crate::ModularTimelineProjection::with_relation_classifier`]; absent
/// one, only kind:1 replies are tallied.
pub trait NoteRelationClassifier: Send + Sync {
    /// Classify `event`, returning the relation it expresses or `None`.
    fn classify(&self, event: &KernelEvent) -> Option<ClassifiedRelation>;
}

pub struct NoteRelationIndex {
    counts: HashMap<String, TargetRelationCounts>,
    relation_by_event: BoundedMessageMap<String, IndexedRelation>,
    /// Cross-protocol relation classifier (reactions/reposts/zaps/comments).
    /// `None` → only NIP-01 kind:1 replies are counted.
    classifier: Option<Arc<dyn NoteRelationClassifier>>,
}

impl Default for NoteRelationIndex {
    fn default() -> Self {
        Self::new(None)
    }
}

impl NoteRelationIndex {
    /// Construct an index with an optional cross-protocol
    /// [`NoteRelationClassifier`]. Pass
    /// `Some(nmp_relations::default_note_relation_classifier())` to count
    /// reactions/reposts/zaps/comments alongside the native kind:1 replies.
    #[must_use]
    pub fn new(classifier: Option<Arc<dyn NoteRelationClassifier>>) -> Self {
        Self {
            counts: HashMap::new(),
            relation_by_event: BoundedMessageMap::new(REPLY_INDEX_CAP),
            classifier,
        }
    }

    #[must_use]
    pub fn counts_for(&self, event_id: &str) -> NoteRelationCounts {
        NoteRelationCounts::for_note(
            event_id,
            self.counts.get(event_id).copied().unwrap_or_default(),
        )
    }

    #[must_use]
    pub fn ingest(&mut self, event: &KernelEvent) -> Vec<String> {
        let Some(relation) = self.classify(event) else {
            return Vec::new();
        };
        if self.relation_by_event.contains_key(&event.id) {
            return Vec::new();
        }
        let (_, evicted) = self
            .relation_by_event
            .insert_returning_evicted(event.id.clone(), relation.clone());
        let mut changed = Vec::new();
        if let Some((_, evicted_relation)) = evicted {
            self.apply_delta(&evicted_relation, Direction::Down);
            changed.push(evicted_relation.target);
        }
        self.apply_delta(&relation, Direction::Up);
        changed.push(relation.target);
        changed.sort();
        changed.dedup();
        changed
    }

    /// Classify an event into the relation it expresses. NIP-01 kind:1 replies
    /// are resolved natively (via this crate's own decoder); every other
    /// relation kind is delegated to the injected cross-protocol classifier.
    fn classify(&self, event: &KernelEvent) -> Option<IndexedRelation> {
        if let Some(note) = try_from_kernel_event(event) {
            return note.refs.reply.or(note.refs.root).map(|reply| IndexedRelation {
                target: reply.id,
                kind: RelationKind::Reply,
            });
        }
        self.classifier
            .as_ref()
            .and_then(|classifier| classifier.classify(event))
            .map(IndexedRelation::from)
    }

    fn apply_delta(&mut self, relation: &IndexedRelation, direction: Direction) {
        let counts = self.counts.entry(relation.target.clone()).or_default();
        let slot = match relation.kind {
            RelationKind::Reply => &mut counts.replies,
            RelationKind::Reaction => &mut counts.reactions,
            RelationKind::Repost => &mut counts.reposts,
            RelationKind::Zap => &mut counts.zaps,
            RelationKind::Comment => &mut counts.comments,
        };
        match direction {
            Direction::Up => *slot = slot.saturating_add(1),
            Direction::Down => *slot = slot.saturating_sub(1),
        }
        if *counts == TargetRelationCounts::default() {
            self.counts.remove(&relation.target);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndexedRelation {
    target: String,
    kind: RelationKind,
}

impl From<ClassifiedRelation> for IndexedRelation {
    fn from(relation: ClassifiedRelation) -> Self {
        Self {
            target: relation.target,
            kind: relation.kind,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    Up,
    Down,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, tags: Vec<Vec<String>>) -> KernelEvent {
        KernelEvent {
            id: id.to_string(),
            author: "a".repeat(64),
            kind: 1,
            created_at: 1,
            tags,
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn counts_direct_replies_without_double_counting_duplicates() {
        let mut index = NoteRelationIndex::default();
        let reply = event(
            "reply",
            vec![vec![
                "e".to_string(),
                "root".to_string(),
                String::new(),
                "reply".to_string(),
            ]],
        );

        assert_eq!(index.ingest(&reply), vec!["root".to_string()]);
        assert!(index.ingest(&reply).is_empty());

        assert_eq!(
            index.counts_for("root").replies,
            RelationCount::Known { count: 1 }
        );
    }

    #[test]
    fn reply_count_is_decremented_when_bounded_map_evicts_oldest_entry() {
        // Use a tiny cap so eviction is easy to trigger in a test.
        const CAP: usize = 2;
        let mut index = NoteRelationIndex {
            counts: std::collections::HashMap::new(),
            relation_by_event: nmp_core::substrate::BoundedMessageMap::new(CAP),
            classifier: None,
        };

        // "reply1" and "reply2" both reply to "root".
        let r1 = event(
            "reply1",
            vec![vec![
                "e".into(),
                "root".into(),
                String::new(),
                "reply".into(),
            ]],
        );
        let r2 = event(
            "reply2",
            vec![vec![
                "e".into(),
                "root".into(),
                String::new(),
                "reply".into(),
            ]],
        );
        // "reply3" replies to "other" — its insertion evicts "reply1" from the bounded map.
        let r3 = event(
            "reply3",
            vec![vec![
                "e".into(),
                "other".into(),
                String::new(),
                "reply".into(),
            ]],
        );

        let _ = index.ingest(&r1);
        let _ = index.ingest(&r2);
        assert_eq!(
            index.counts_for("root").replies,
            RelationCount::Known { count: 2 }
        );

        // r3 pushes r1 out: "root" count should drop to 1.
        let _ = index.ingest(&r3);
        assert_eq!(
            index.counts_for("root").replies,
            RelationCount::Known { count: 1 },
            "evicting an old reply must decrement the parent's count"
        );
        assert_eq!(
            index.counts_for("other").replies,
            RelationCount::Known { count: 1 },
        );
    }

    #[test]
    fn counts_distinguish_known_zero_from_loading_interests() {
        let counts = NoteRelationIndex::default().counts_for("root");

        assert_eq!(counts.replies, RelationCount::Known { count: 0 });
        assert_eq!(counts.reactions, RelationCount::Known { count: 0 });
        assert_eq!(counts.reposts, RelationCount::Known { count: 0 });
        assert_eq!(counts.zaps, RelationCount::Known { count: 0 });
        assert_eq!(counts.comments, RelationCount::Known { count: 0 });
    }

    /// Cross-protocol classification (reactions/reposts/zaps/comments) is owned
    /// by `nmp-relations`; this test verifies the seam — an injected classifier
    /// drives the non-reply buckets. The concrete cross-protocol classifier and
    /// its NIP-22/18/57 coverage are tested in `nmp-relations`.
    #[test]
    fn injected_classifier_drives_cross_protocol_counts() {
        struct AlwaysRepost;
        impl NoteRelationClassifier for AlwaysRepost {
            fn classify(&self, evt: &KernelEvent) -> Option<ClassifiedRelation> {
                // kind:1 replies are native; classify everything else as a repost.
                if evt.kind == 1 {
                    return None;
                }
                Some(ClassifiedRelation {
                    target: "root".to_string(),
                    kind: RelationKind::Repost,
                })
            }
        }
        let mut index = NoteRelationIndex::new(Some(Arc::new(AlwaysRepost)));
        let mut repost = event("repost", Vec::new());
        repost.kind = 6;
        assert_eq!(index.ingest(&repost), vec!["root".to_string()]);
        assert_eq!(
            index.counts_for("root").reposts,
            RelationCount::Known { count: 1 }
        );
    }
}
