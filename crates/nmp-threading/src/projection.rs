//! Reactive threading graph projection.
//!
//! `ThreadingProjection` is the read-model half of the threading mechanism:
//! callers open an observed projection for a concrete event scope, this sink
//! folds accepted events through a [`ParentResolver`], and the snapshot side
//! emits typed edge rows plus the grouped block layout.

use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use serde::{Deserialize, Serialize};

use crate::runtime::THREADING_GRAPH_SCHEMA_ID;
use crate::wire::{
    encode_threading_snapshot, THREADING_GRAPH_FILE_IDENTIFIER, THREADING_GRAPH_SCHEMA_VERSION,
};
use crate::{
    EtagThreadResolver, Grouper, ModulePolicy, ParentResolver, ThreadPointer, TimelineBlock,
};

/// Per-event threading facts keyed by event id.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadEdge {
    /// Event id this row describes.
    pub event_id: String,
    /// Author pubkey copied from the event.
    pub author_pubkey: String,
    /// Raw event kind copied from the event. The projection does not branch on it.
    pub kind: u32,
    /// Event creation time as Unix seconds.
    pub created_at: u64,
    /// Direct parent resolved from `e` tag grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ThreadPointer>,
    /// Thread root resolved from `e` tag grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<ThreadPointer>,
    /// Best-effort parent-author hint, usually the first accompanying `p` tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_author_pubkey: Option<String>,
}

/// Typed snapshot emitted by the `nmp.threading.graph.*` projection family.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadingSnapshot {
    /// Stable edge rows ordered by event id.
    pub edges: Vec<ThreadEdge>,
    /// Display-order grouping blocks, newest block first.
    pub blocks: Vec<TimelineBlock>,
    /// Ancestor event ids the local scope references but has not seen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_ancestor_ids: Vec<String>,
}

impl ThreadingSnapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Kind-blind observed projection over a scoped event stream.
pub struct ThreadingProjection<R: ParentResolver = EtagThreadResolver> {
    state: Mutex<ThreadingState<R>>,
}

struct ThreadingState<R: ParentResolver> {
    resolver: R,
    grouper: Grouper<R>,
    edges: BoundedMessageMap<String, ThreadEdge>,
}

impl ThreadingProjection<EtagThreadResolver> {
    /// Construct the default e-tag threading projection.
    #[must_use]
    pub fn etag(policy: ModulePolicy) -> Self {
        Self::new(EtagThreadResolver, policy, MAX_PROJECTION_MESSAGES)
    }
}

impl<R: ParentResolver + Clone> ThreadingProjection<R> {
    /// Construct a projection using a caller-supplied resolver.
    #[must_use]
    pub fn new(resolver: R, policy: ModulePolicy, capacity: usize) -> Self {
        let grouper = Grouper::new(resolver.clone(), policy);
        Self {
            state: Mutex::new(ThreadingState {
                resolver,
                grouper,
                edges: BoundedMessageMap::new(capacity),
            }),
        }
    }

    /// Return a typed snapshot. D6: poisoned state degrades to empty.
    #[must_use]
    pub fn snapshot(&self) -> ThreadingSnapshot {
        self.state
            .lock()
            .map(|state| state.snapshot())
            .unwrap_or_else(|_| ThreadingSnapshot::empty())
    }

    /// Build typed projection data for `projection_key`.
    #[must_use]
    pub fn typed_projection(&self, projection_key: &str) -> TypedProjectionData {
        let snapshot = self.snapshot();
        TypedProjectionData {
            key: projection_key.to_string(),
            schema_id: THREADING_GRAPH_SCHEMA_ID.to_string(),
            schema_version: THREADING_GRAPH_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(THREADING_GRAPH_FILE_IDENTIFIER).into_owned(),
            payload: encode_threading_snapshot(&snapshot),
            ..Default::default()
        }
    }
}

impl<R: ParentResolver + Clone> ObservedProjectionSink for ThreadingProjection<R> {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if let Ok(mut state) = self.state.lock() {
            state.ingest(event);
        }
    }
}

impl<R: ParentResolver + Clone> ThreadingState<R> {
    fn ingest(&mut self, event: &KernelEvent) {
        let known = self.edges.contains_key(event.id.as_str());
        if !known {
            self.evict_oldest_if_full();
        }
        let edge = ThreadEdge {
            event_id: event.id.clone(),
            author_pubkey: event.author.clone(),
            kind: event.kind,
            created_at: event.created_at,
            parent: self.resolver.parent(event),
            root: self.resolver.root(event),
            parent_author_pubkey: self.resolver.parent_author(event),
        };
        if known {
            let _ = self.grouper.on_replace(&event.id, event);
        } else {
            let _ = self.grouper.on_insert(event);
        }
        self.edges.insert(edge.event_id.clone(), edge);
    }

    fn evict_oldest_if_full(&mut self) {
        if self.edges.len() < self.edges.capacity() {
            return;
        }
        let Some((oldest_id, _)) = self.edges.first() else {
            return;
        };
        let oldest_id = oldest_id.clone();
        self.edges.remove(oldest_id.as_str());
        let _ = self.grouper.on_remove(&oldest_id);
    }

    fn snapshot(&self) -> ThreadingSnapshot {
        let mut edges: Vec<ThreadEdge> = self.edges.values().cloned().collect();
        edges.sort_by(|a, b| a.event_id.cmp(&b.event_id));
        ThreadingSnapshot {
            edges,
            blocks: self.grouper.blocks().to_vec(),
            pending_ancestor_ids: self
                .grouper
                .pending_ancestor_ids()
                .iter()
                .cloned()
                .collect(),
        }
    }
}
