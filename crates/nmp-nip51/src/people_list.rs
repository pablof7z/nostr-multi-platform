//! `PeopleListProjection` -- the active account's NIP-51 kind:30000 follow
//! sets (people lists), keyed by their `d`-tag identifier.
//!
//! The perspective compiler's `ListMembers { list }` scope resolves a list id
//! to its member pubkeys through this projection. The projection owns the
//! reactive source graph for `(active account, replaceable NIP-51 list store)`
//! and emits source effects when visible list membership changes. Feed sessions
//! consume those effects to replace dependent acquisition, reset the window,
//! and replay the already-open feed without app-side cache invalidation.
//!
//! Public `p` tags are the v1 source. Private encrypted NIP-51 members remain
//! out of scope because this read-only projection crate has no signer/decrypt
//! capability.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, RwLock};

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_kinds::KIND_FOLLOW_SET;
use serde::Serialize;

use crate::people_list_graph::{PeopleListGraph, PeopleListGraphEffect};

/// Snapshot of one follow set's members (diagnostic / export).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PeopleListSnapshot {
    /// The list's `d`-tag identifier.
    pub list_id: String,
    /// The member pubkeys (lowercase hex), sorted.
    pub members: Vec<String>,
}

/// Source effect emitted after the people-list graph proves that the active
/// account's visible list membership changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeopleListSourceEffect {
    PerspectiveChanged {
        lists: BTreeMap<String, BTreeSet<String>>,
    },
}

/// A registered graph source-effect sink. Feed sessions use this to reconcile
/// acquisition, observed projections, and visible rows for `ListMembers`.
pub type PeopleListSourceEffectSink = Box<dyn Fn(&PeopleListSourceEffect) + Send + Sync>;

/// The active account's kind:30000 follow sets, keyed by `d`-tag.
///
/// Construct with the shared `active_pubkey` slot and register the same `Arc`
/// as an [`ObservedProjectionSink`] so kind:30000 events are ingested. The graph
/// is the only writer that decides when `visible_lists` changes.
pub struct PeopleListProjection {
    active_pubkey: Arc<Mutex<Option<String>>>,
    graph: Mutex<PeopleListGraph>,
    visible_lists: Arc<RwLock<BTreeMap<String, BTreeSet<String>>>>,
    source_effect_sinks: Mutex<Vec<PeopleListSourceEffectSink>>,
}

impl PeopleListProjection {
    /// Construct with a shared `active_pubkey` slot.
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        let graph = PeopleListGraph::new(active_pubkey_from_slot(&active_pubkey));
        let visible_lists = graph.current_visible_lists();
        Self {
            active_pubkey,
            graph: Mutex::new(graph),
            visible_lists: Arc::new(RwLock::new(visible_lists)),
            source_effect_sinks: Mutex::new(Vec::new()),
        }
    }

    /// Register an internal source-effect sink fired after the source graph
    /// proves the active account's visible list perspective changed.
    pub fn on_source_effect(&self, sink: PeopleListSourceEffectSink) {
        if let Ok(mut sinks) = self.source_effect_sinks.lock() {
            sinks.push(sink);
        }
    }

    /// Notify the projection that the active account changed.
    ///
    /// Re-reads the active-account slot, updates the graph's active-account
    /// source, and emits a source effect when visible list membership changes.
    pub fn notify_account_changed(&self) {
        let active = active_pubkey_from_slot(&self.active_pubkey);
        let effects = match self.graph.lock() {
            Ok(mut graph) => graph.apply_active_source(active),
            Err(_) => Vec::new(),
        };
        self.apply_graph_effects(effects);
    }

    /// The members of the active account's follow set identified by `list_id`
    /// (its `d`-tag), sorted lowercase hex.
    ///
    /// Returns the empty set when no active account/list is visible or a lock is
    /// poisoned. The perspective compiler treats empty membership as
    /// fail-closed: admit nobody and acquire no member timelines.
    #[must_use]
    pub fn members(&self, list_id: &str) -> BTreeSet<String> {
        self.visible_lists
            .read()
            .ok()
            .and_then(|lists| lists.get(list_id).cloned())
            .unwrap_or_default()
    }

    /// A diagnostic snapshot of one list.
    #[must_use]
    pub fn snapshot(&self, list_id: &str) -> PeopleListSnapshot {
        PeopleListSnapshot {
            list_id: list_id.to_string(),
            members: self.members(list_id).into_iter().collect(),
        }
    }

    fn apply_graph_effects(&self, effects: Vec<PeopleListGraphEffect>) {
        let mut source_effects = Vec::new();
        for effect in effects {
            match effect {
                PeopleListGraphEffect::PerspectiveChanged { lists } => {
                    if self.replace_visible_lists(lists.clone()) {
                        source_effects.push(PeopleListSourceEffect::PerspectiveChanged { lists });
                    }
                }
            }
        }
        if !source_effects.is_empty() {
            self.fire_source_effects(&source_effects);
        }
    }

    fn replace_visible_lists(&self, rebuilt: BTreeMap<String, BTreeSet<String>>) -> bool {
        match self.visible_lists.write() {
            Ok(mut guard) => {
                *guard = rebuilt;
                true
            }
            Err(_) => false,
        }
    }

    fn fire_source_effects(&self, effects: &[PeopleListSourceEffect]) {
        let sinks = match self.source_effect_sinks.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        for effect in effects {
            for sink in sinks.iter() {
                sink(effect);
            }
        }
    }
}

fn active_pubkey_from_slot(slot: &Arc<Mutex<Option<String>>>) -> Option<String> {
    match slot.lock() {
        Ok(guard) => guard.as_ref().cloned(),
        Err(_) => None,
    }
}

impl ObservedProjectionSink for PeopleListProjection {
    /// Called by the kernel once per accepted kind:30000 event.
    ///
    /// Gate by `kind == 30000` and author == active pubkey, read the `d`-tag,
    /// extract public `p` members, and upsert the graph source. Newer
    /// addressable-replaceable events replace older ones; older echoes are
    /// graph no-ops and emit no source effect.
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_FOLLOW_SET {
            return;
        }
        let active = match active_pubkey_from_slot(&self.active_pubkey) {
            Some(active) => active,
            None => return,
        };
        if active != event.author {
            return;
        }

        let list_id = event
            .tags
            .iter()
            .find(|tag| tag.first().is_some_and(|t| t == "d"))
            .and_then(|tag| tag.get(1).cloned())
            .unwrap_or_default();

        let members: BTreeSet<String> = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().is_some_and(|t| t == "p") {
                    tag.get(1).cloned()
                } else {
                    None
                }
            })
            .collect();

        let effects = match self.graph.lock() {
            Ok(mut graph) => {
                graph.upsert_list(event.author.clone(), list_id, members, event.created_at)
            }
            Err(_) => Vec::new(),
        };
        self.apply_graph_effects(effects);
    }
}

#[cfg(test)]
#[path = "people_list_tests.rs"]
mod tests;
