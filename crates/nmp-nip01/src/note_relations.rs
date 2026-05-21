use std::collections::HashMap;

use nmp_core::substrate::KernelEvent;
use serde::{Deserialize, Serialize};

use crate::decode::try_from_kernel_event;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteRelationCounts {
    pub replies: RelationCount,
    pub reactions: RelationCount,
    pub reposts: RelationCount,
}

impl NoteRelationCounts {
    pub fn for_note(event_id: &str, replies: u64) -> Self {
        Self {
            replies: RelationCount::known(replies),
            reactions: RelationCount::loading(RelationCountInterest::reactions(event_id)),
            reposts: RelationCount::loading(RelationCountInterest::reposts(event_id)),
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
    pub fn known(count: u64) -> Self {
        Self::Known { count }
    }

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
    pub fn reactions(event_id: &str) -> Self {
        Self {
            namespace: "nmp.reactions.summary".to_string(),
            target_event_id: event_id.to_string(),
            tag: "e".to_string(),
        }
    }

    pub fn reposts(event_id: &str) -> Self {
        Self {
            namespace: "nmp.reactions.reposts".to_string(),
            target_event_id: event_id.to_string(),
            tag: "e".to_string(),
        }
    }
}

#[derive(Default)]
pub struct NoteRelationIndex {
    reply_counts: HashMap<String, u64>,
    reply_parent_by_event: HashMap<String, String>,
}

impl NoteRelationIndex {
    pub fn counts_for(&self, event_id: &str) -> NoteRelationCounts {
        NoteRelationCounts::for_note(event_id, self.reply_count_for(event_id))
    }

    pub fn ingest(&mut self, event: &KernelEvent) -> Vec<String> {
        let Some(note) = try_from_kernel_event(event) else {
            return Vec::new();
        };
        let Some(parent) = note.refs.reply.or(note.refs.root).map(|reply| reply.id) else {
            return Vec::new();
        };
        if self.reply_parent_by_event.contains_key(&event.id) {
            return Vec::new();
        }
        self.reply_parent_by_event
            .insert(event.id.clone(), parent.clone());
        let count = self.reply_counts.entry(parent.clone()).or_insert(0);
        *count = count.saturating_add(1);
        vec![parent]
    }

    fn reply_count_for(&self, event_id: &str) -> u64 {
        self.reply_counts.get(event_id).copied().unwrap_or(0)
    }
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
    fn counts_distinguish_known_zero_from_loading_interests() {
        let counts = NoteRelationIndex::default().counts_for("root");

        assert_eq!(counts.replies, RelationCount::Known { count: 0 });
        assert!(matches!(
            counts.reactions,
            RelationCount::Loading { ref interest }
                if interest.namespace == "nmp.reactions.summary"
                    && interest.target_event_id == "root"
        ));
        assert!(matches!(
            counts.reposts,
            RelationCount::Loading { ref interest }
                if interest.namespace == "nmp.reactions.reposts"
                    && interest.target_event_id == "root"
        ));
    }
}
