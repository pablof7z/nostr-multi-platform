use std::collections::HashMap;

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
}

impl NoteRelationCounts {
    #[must_use]
    pub fn for_note(_event_id: &str, counts: TargetRelationCounts) -> Self {
        Self {
            replies: RelationCount::known(counts.replies),
            reactions: RelationCount::known(counts.reactions),
            reposts: RelationCount::known(counts.reposts),
            zaps: RelationCount::known(counts.zaps),
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
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TargetRelationCounts {
    pub replies: u64,
    pub reactions: u64,
    pub reposts: u64,
    pub zaps: u64,
}

pub struct NoteRelationIndex {
    counts: HashMap<String, TargetRelationCounts>,
    relation_by_event: BoundedMessageMap<String, IndexedRelation>,
}

impl Default for NoteRelationIndex {
    fn default() -> Self {
        Self {
            counts: HashMap::new(),
            relation_by_event: BoundedMessageMap::new(REPLY_INDEX_CAP),
        }
    }
}

impl NoteRelationIndex {
    #[must_use]
    pub fn counts_for(&self, event_id: &str) -> NoteRelationCounts {
        NoteRelationCounts::for_note(
            event_id,
            self.counts.get(event_id).copied().unwrap_or_default(),
        )
    }

    #[must_use]
    pub fn ingest(&mut self, event: &KernelEvent) -> Vec<String> {
        let Some(relation) = IndexedRelation::from_event(event) else {
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

    fn apply_delta(&mut self, relation: &IndexedRelation, direction: Direction) {
        let counts = self.counts.entry(relation.target.clone()).or_default();
        let slot = match relation.kind {
            RelationKind::Reply => &mut counts.replies,
            RelationKind::Reaction => &mut counts.reactions,
            RelationKind::Repost => &mut counts.reposts,
            RelationKind::Zap => &mut counts.zaps,
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

impl IndexedRelation {
    fn from_event(event: &KernelEvent) -> Option<Self> {
        if let Some(note) = try_from_kernel_event(event) {
            return note.refs.reply.or(note.refs.root).map(|reply| Self {
                target: reply.id,
                kind: RelationKind::Reply,
            });
        }
        if event.kind == nmp_nip18::KIND_REPOST {
            return nmp_nip18::try_from_kernel_event(event)
                .and_then(|repost| repost.target_event_id)
                .map(|target| Self {
                    target,
                    kind: RelationKind::Repost,
                });
        }
        if event.kind == 7 {
            return first_event_tag(&event.tags).map(|target| Self {
                target,
                kind: RelationKind::Reaction,
            });
        }
        nmp_nip57::try_from_kernel_event(event)
            .and_then(|zap| zap.zapped_event_id)
            .map(|target| Self {
                target,
                kind: RelationKind::Zap,
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RelationKind {
    Reply,
    Reaction,
    Repost,
    Zap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    Up,
    Down,
}

fn first_event_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "e") {
            tag.get(1).filter(|id| !id.is_empty()).cloned()
        } else {
            None
        }
    })
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
    }
}
