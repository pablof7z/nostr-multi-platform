use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::ObservedProjectionSink;
use serde::{Deserialize, Serialize};

use crate::action::{KIND_REACTION, KIND_REACTION_DELETE};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReactionEntry {
    pub reaction_event_id: String,
    pub target_event_id: String,
    pub author_pubkey: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_author_pubkey: Option<String>,
    pub content: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ViewerReactionState {
    pub target_event_id: String,
    pub viewer_pubkey: String,
    pub reaction_event_id: String,
    pub content: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReactionSnapshot {
    pub target_event_id: String,
    pub reactions: Vec<ReactionEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_reaction: Option<ViewerReactionState>,
}

pub struct ReactionProjection {
    viewer_pubkey: Mutex<Option<String>>,
    entries: Mutex<BoundedMessageMap<String, ReactionEntry>>,
}

impl ReactionProjection {
    #[must_use]
    pub fn new(viewer_pubkey: Option<String>) -> Self {
        Self {
            viewer_pubkey: Mutex::new(viewer_pubkey),
            entries: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    pub fn set_viewer_pubkey(&self, viewer_pubkey: Option<String>) {
        if let Ok(mut current) = self.viewer_pubkey.lock() {
            *current = viewer_pubkey;
        }
    }

    #[must_use]
    pub fn snapshot_for(&self, target_event_id: &str) -> ReactionSnapshot {
        let Ok(entries) = self.entries.lock() else {
            return ReactionSnapshot {
                target_event_id: target_event_id.to_string(),
                ..Default::default()
            };
        };
        let mut reactions: Vec<ReactionEntry> = entries
            .values()
            .filter(|entry| entry.target_event_id == target_event_id)
            .cloned()
            .collect();
        reactions.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.reaction_event_id.cmp(&b.reaction_event_id))
        });
        let viewer_reaction = self
            .viewer_pubkey
            .lock()
            .ok()
            .and_then(|viewer| viewer.clone())
            .and_then(|viewer| latest_viewer_reaction(&reactions, target_event_id, &viewer));
        ReactionSnapshot {
            target_event_id: target_event_id.to_string(),
            reactions,
            viewer_reaction,
        }
    }

    #[must_use]
    pub fn viewer_reaction(
        &self,
        target_event_id: &str,
        viewer_pubkey: &str,
    ) -> Option<ViewerReactionState> {
        let Ok(entries) = self.entries.lock() else {
            return None;
        };
        latest_viewer_reaction(
            &entries.values().cloned().collect::<Vec<_>>(),
            target_event_id,
            viewer_pubkey,
        )
    }

    fn ingest(&self, event: &KernelEvent) {
        match event.kind {
            KIND_REACTION => self.ingest_reaction(event),
            KIND_REACTION_DELETE => self.ingest_delete(event),
            _ => {}
        }
    }

    fn ingest_reaction(&self, event: &KernelEvent) {
        let Some(target_event_id) = first_tag_value(&event.tags, "e") else {
            return;
        };
        let entry = ReactionEntry {
            reaction_event_id: event.id.clone(),
            target_event_id,
            author_pubkey: event.author.clone(),
            target_author_pubkey: first_tag_value(&event.tags, "p"),
            content: if event.content.trim().is_empty() {
                "+".to_string()
            } else {
                event.content.clone()
            },
            created_at: event.created_at,
        };
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(entry.reaction_event_id.clone(), entry);
        }
    }

    fn ingest_delete(&self, event: &KernelEvent) {
        // Delegate `e`-tag parsing to the nmp-nip09 read seam so tag grammar
        // interpretation is centralised in the deletion owner (ADR-0074).
        let deleted_ids = nmp_nip09::deletion_targets(&event.tags).event_ids;
        if deleted_ids.is_empty() {
            return;
        }
        if let Ok(mut entries) = self.entries.lock() {
            for id in deleted_ids {
                if entries
                    .get(&id)
                    .is_some_and(|entry| entry.author_pubkey == event.author)
                {
                    entries.remove(&id);
                }
            }
        }
    }
}

impl Default for ReactionProjection {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ObservedProjectionSink for ReactionProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ingest(event);
    }
}

fn latest_viewer_reaction(
    entries: &[ReactionEntry],
    target_event_id: &str,
    viewer_pubkey: &str,
) -> Option<ViewerReactionState> {
    entries
        .iter()
        .filter(|entry| {
            entry.target_event_id == target_event_id && entry.author_pubkey == viewer_pubkey
        })
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.reaction_event_id.cmp(&b.reaction_event_id))
        })
        .map(|entry| ViewerReactionState {
            target_event_id: entry.target_event_id.clone(),
            viewer_pubkey: viewer_pubkey.to_string(),
            reaction_event_id: entry.reaction_event_id.clone(),
            content: entry.content.clone(),
            created_at: entry.created_at,
        })
}

fn first_tag_value(tags: &[Vec<String>], name: &str) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|candidate| candidate == name) {
            tag.get(1).filter(|value| !value.is_empty()).cloned()
        } else {
            None
        }
    })
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
