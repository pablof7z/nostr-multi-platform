//! Threaded NIP-22 comment projection.
//!
//! [`CommentThreadProjection`] is a [`KernelEventObserver`] (the same in-memory
//! read-model shape `nmp-nip25` reactions and `nmp-nip51` bookmarks use). It
//! ingests kind:1111 events, buckets them by root scope value, and on demand
//! builds the parent/child forest for one root.
//!
//! It holds raw data only. Reply counts, "view N more replies" strings, nav
//! titles, and SF Symbols are presentation and belong in the shell (D1).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::KernelEventObserver;
use serde::{Deserialize, Serialize};

use crate::decode::{try_from_kernel_event, CommentRecord};

/// One node in a comment forest: a record plus its child replies, oldest-first.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommentThreadNode {
    pub record: CommentRecord,
    pub children: Vec<CommentThreadNode>,
}

/// A read-only snapshot of one root's comment thread: the flat record set plus
/// the built forest. Raw data only.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommentThreadSnapshot {
    /// Root scope value the snapshot was taken for (`E`/`A`/`I` value).
    pub root_tag_value: String,
    /// All comments observed for this root, newest-first.
    pub records: Vec<CommentRecord>,
    /// Parent/child forest built from `records`, children oldest-first.
    pub tree: Vec<CommentThreadNode>,
}

/// In-memory kind:1111 comment read model, bucketed by root scope value.
pub struct CommentThreadProjection {
    entries: Mutex<BoundedMessageMap<String, CommentRecord>>,
}

impl CommentThreadProjection {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// Snapshot the comment thread rooted at `root_tag_value`. Records are
    /// returned newest-first; the tree's children are oldest-first.
    #[must_use]
    pub fn snapshot_for(&self, root_tag_value: &str) -> CommentThreadSnapshot {
        let Ok(entries) = self.entries.lock() else {
            return CommentThreadSnapshot {
                root_tag_value: root_tag_value.to_string(),
                ..Default::default()
            };
        };
        let mut records: Vec<CommentRecord> = entries
            .values()
            .filter(|record| record.root_tag_value == root_tag_value)
            .cloned()
            .collect();
        records.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.event_id.cmp(&b.event_id))
        });
        let tree = build_thread(&records, root_tag_value);
        CommentThreadSnapshot {
            root_tag_value: root_tag_value.to_string(),
            records,
            tree,
        }
    }

    fn ingest(&self, event: &KernelEvent) {
        let Some(record) = try_from_kernel_event(event) else {
            return;
        };
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(record.event_id.clone(), record);
        }
    }
}

impl Default for CommentThreadProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelEventObserver for CommentThreadProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ingest(event);
    }
}

/// Build a nested comment forest from a bounded record set.
///
/// Children are oldest-first. Comments whose parent is not present in the
/// bounded input are promoted to the top level so fetched content stays
/// visible rather than disappearing. Recursive parent edges are broken so a
/// malformed thread cannot cause unbounded recursion.
#[must_use]
pub fn build_thread(records: &[CommentRecord], root_tag_value: &str) -> Vec<CommentThreadNode> {
    if records.is_empty() {
        return Vec::new();
    }

    let mut sorted = records.to_vec();
    sorted.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.event_id.cmp(&b.event_id))
    });

    let mut by_parent: HashMap<String, Vec<CommentRecord>> = HashMap::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    for record in &sorted {
        by_parent
            .entry(record.parent_tag_value.clone())
            .or_default()
            .push(record.clone());
        seen_ids.insert(record.event_id.clone());
    }

    let mut top_level = by_parent.get(root_tag_value).cloned().unwrap_or_default();
    for record in &sorted {
        let parent = record.parent_tag_value.as_str();
        if parent == root_tag_value || seen_ids.contains(parent) {
            continue;
        }
        top_level.push(record.clone());
    }

    let mut path = HashSet::new();
    top_level
        .into_iter()
        .map(|record| build_node(record, &by_parent, &mut path))
        .collect()
}

fn build_node(
    record: CommentRecord,
    by_parent: &HashMap<String, Vec<CommentRecord>>,
    path: &mut HashSet<String>,
) -> CommentThreadNode {
    if !path.insert(record.event_id.clone()) {
        return CommentThreadNode {
            record,
            children: Vec::new(),
        };
    }

    let child_records: Vec<CommentRecord> = by_parent
        .get(&record.event_id)
        .map(|records| {
            records
                .iter()
                .filter(|child| !path.contains(&child.event_id))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let children = child_records
        .into_iter()
        .map(|child| build_node(child, by_parent, path))
        .collect();
    path.remove(&record.event_id);

    CommentThreadNode { record, children }
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
