//! Active account's NIP-51 kind:10009 simple-groups list.
//!
//! NIP-51 owns the replaceable list parser and source graph. NIP-29 owns group
//! routing semantics; this projection exposes only the typed public group refs
//! that a feed-session compiler can turn into host-pinned group interests.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, RwLock};

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_kinds::KIND_SIMPLE_GROUPS;
use serde::Serialize;

use crate::group_list_graph::{SimpleGroupListGraph, SimpleGroupListGraphEffect};

/// A public NIP-51 simple-group reference from kind:10009.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SimpleGroupRef {
    /// NIP-29 local group id, stored in the list's `group` tag.
    pub local_id: String,
    /// Host relay URL from the list's `group` tag.
    pub host_relay_url: String,
}

impl SimpleGroupRef {
    #[must_use]
    pub fn new(local_id: impl Into<String>, host_relay_url: impl Into<String>) -> Self {
        Self {
            local_id: local_id.into(),
            host_relay_url: host_relay_url.into(),
        }
    }

    #[must_use]
    pub fn is_routable(&self) -> bool {
        !self.local_id.is_empty() && !self.host_relay_url.is_empty()
    }
}

/// Snapshot of the active account's public simple-group refs.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SimpleGroupListSnapshot {
    pub groups: Vec<SimpleGroupRef>,
}

/// Source effect emitted after the graph proves the visible simple-group set
/// changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimpleGroupListSourceEffect {
    PerspectiveChanged { groups: BTreeSet<SimpleGroupRef> },
}

pub type SimpleGroupListSourceEffectSink = Box<dyn Fn(&SimpleGroupListSourceEffect) + Send + Sync>;

/// Active account's kind:10009 simple-groups list.
pub struct SimpleGroupListProjection {
    active_pubkey: Arc<Mutex<Option<String>>>,
    graph: Mutex<SimpleGroupListGraph>,
    visible_groups: Arc<RwLock<BTreeSet<SimpleGroupRef>>>,
    source_effect_sinks: Mutex<Vec<SimpleGroupListSourceEffectSink>>,
}

impl SimpleGroupListProjection {
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        let graph = SimpleGroupListGraph::new(active_pubkey_from_slot(&active_pubkey));
        let visible_groups = graph.current_visible_groups();
        Self {
            active_pubkey,
            graph: Mutex::new(graph),
            visible_groups: Arc::new(RwLock::new(visible_groups)),
            source_effect_sinks: Mutex::new(Vec::new()),
        }
    }

    pub fn on_source_effect(&self, sink: SimpleGroupListSourceEffectSink) {
        if let Ok(mut sinks) = self.source_effect_sinks.lock() {
            sinks.push(sink);
        }
    }

    pub fn notify_account_changed(&self) {
        let active = active_pubkey_from_slot(&self.active_pubkey);
        let effects = match self.graph.lock() {
            Ok(mut graph) => graph.apply_active_source(active),
            Err(_) => Vec::new(),
        };
        self.apply_graph_effects(effects);
    }

    #[must_use]
    pub fn groups(&self) -> BTreeSet<SimpleGroupRef> {
        self.visible_groups
            .read()
            .map(|groups| groups.clone())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn snapshot(&self) -> SimpleGroupListSnapshot {
        SimpleGroupListSnapshot {
            groups: self.groups().into_iter().collect(),
        }
    }

    fn apply_graph_effects(&self, effects: Vec<SimpleGroupListGraphEffect>) {
        let mut source_effects = Vec::new();
        for effect in effects {
            match effect {
                SimpleGroupListGraphEffect::PerspectiveChanged { groups } => {
                    if self.replace_visible_groups(groups.clone()) {
                        source_effects
                            .push(SimpleGroupListSourceEffect::PerspectiveChanged { groups });
                    }
                }
            }
        }
        if !source_effects.is_empty() {
            self.fire_source_effects(&source_effects);
        }
    }

    fn replace_visible_groups(&self, rebuilt: BTreeSet<SimpleGroupRef>) -> bool {
        match self.visible_groups.write() {
            Ok(mut guard) => {
                *guard = rebuilt;
                true
            }
            Err(_) => false,
        }
    }

    fn fire_source_effects(&self, effects: &[SimpleGroupListSourceEffect]) {
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

impl ObservedProjectionSink for SimpleGroupListProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_SIMPLE_GROUPS {
            return;
        }
        let active = match active_pubkey_from_slot(&self.active_pubkey) {
            Some(active) => active,
            None => return,
        };
        if active != event.author {
            return;
        }

        let groups = simple_groups_from_tags(&event.tags);
        let effects = match self.graph.lock() {
            Ok(mut graph) => graph.upsert_list(event.author.clone(), groups, event.created_at),
            Err(_) => Vec::new(),
        };
        self.apply_graph_effects(effects);
    }
}

#[must_use]
pub fn simple_groups_from_tags(tags: &[Vec<String>]) -> BTreeSet<SimpleGroupRef> {
    tags.iter()
        .filter_map(|tag| {
            if tag.first().map(String::as_str) != Some("group") {
                return None;
            }
            let relay_url = nmp_core::canonical_relay_url(tag.get(2)?)?;
            let group = SimpleGroupRef::new(tag.get(1)?.clone(), relay_url);
            group.is_routable().then_some(group)
        })
        .collect()
}

#[cfg(test)]
#[path = "group_list_tests.rs"]
mod tests;
