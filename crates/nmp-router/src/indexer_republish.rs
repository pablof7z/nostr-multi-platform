//! Indexer-republish target-selection policy.
//!
//! The policy passively forwards accepted replaceable events from non-indexer
//! relays to configured indexer relays. `nmp-core` owns the generic observer
//! and pool send; this crate owns the routing/provenance decision.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use nmp_core::slots::IndexerRelaysSlot;
use nmp_core::store::{EventStore, RawEvent};
use nmp_core::substrate::{
    RawEventForwardPolicy, RawEventForwardPolicyContext, RawEventForwardTarget,
};
use nmp_core::{KindFilter, RelayRole};

const DEDUP_CAPACITY: usize = 4096;

/// Policy for forwarding replaceable events to indexer relays.
pub struct IndexerRepublishPolicy {
    enabled: bool,
    indexer_relays: IndexerRelaysSlot,
    store: Arc<dyn EventStore>,
    dedup: Mutex<DedupCache>,
}

struct DedupCache {
    order: VecDeque<(String, String)>,
    set: HashSet<(String, String)>,
}

impl DedupCache {
    fn new() -> Self {
        Self {
            order: VecDeque::with_capacity(DEDUP_CAPACITY),
            set: HashSet::with_capacity(DEDUP_CAPACITY),
        }
    }

    fn insert(&mut self, key: (String, String)) -> bool {
        if self.set.contains(&key) {
            return false;
        }
        if self.order.len() >= DEDUP_CAPACITY {
            if let Some(victim) = self.order.pop_front() {
                self.set.remove(&victim);
            }
        }
        self.set.insert(key.clone());
        self.order.push_back(key);
        true
    }
}

impl IndexerRepublishPolicy {
    #[must_use]
    pub fn new(enabled: bool, context: RawEventForwardPolicyContext) -> Self {
        Self {
            enabled,
            indexer_relays: context.indexer_relays,
            store: context.event_store,
            dedup: Mutex::new(DedupCache::new()),
        }
    }

    #[must_use]
    pub fn enabled(context: RawEventForwardPolicyContext) -> Self {
        Self::new(true, context)
    }

    #[must_use]
    pub fn replaceable_kind_filter() -> KindFilter {
        let kinds = std::iter::once(0u32)
            .chain(std::iter::once(3u32))
            .chain(10_000u32..20_000u32);
        KindFilter::from_kinds(kinds)
    }

    fn indexer_urls(&self) -> Vec<String> {
        self.indexer_relays
            .lock()
            .map(|guard| guard.as_slice().to_vec())
            .unwrap_or_default()
    }

    fn event_has_indexer_provenance(&self, raw: &RawEvent, indexer_urls: &[String]) -> bool {
        self.store
            .provenance_for(&raw.id_bytes())
            .map(|entries| {
                entries
                    .iter()
                    .any(|entry| indexer_urls.iter().any(|url| *url == entry.relay_url))
            })
            .unwrap_or(false)
    }
}

impl RawEventForwardPolicy for IndexerRepublishPolicy {
    fn kind_filter(&self) -> KindFilter {
        Self::replaceable_kind_filter()
    }

    fn forward_targets(
        &self,
        raw: &RawEvent,
        source_relay_url: Option<&str>,
    ) -> Vec<RawEventForwardTarget> {
        if !self.enabled || !raw.is_replaceable() {
            return Vec::new();
        }

        let indexer_urls = self.indexer_urls();
        if indexer_urls.is_empty() {
            return Vec::new();
        }

        if let Some(source) = source_relay_url {
            if indexer_urls.iter().any(|url| url == source) {
                return Vec::new();
            }
        }

        if self.event_has_indexer_provenance(raw, &indexer_urls) {
            return Vec::new();
        }

        let mut targets = Vec::new();
        for target in indexer_urls {
            if source_relay_url.is_some_and(|source| source == target.as_str()) {
                continue;
            }
            let key = (raw.id.clone(), target.clone());
            let should_forward = self
                .dedup
                .lock()
                .map(|mut guard| guard.insert(key))
                .unwrap_or(false);
            if should_forward {
                targets.push(RawEventForwardTarget::new(target, RelayRole::Indexer));
            }
        }
        targets
    }
}

#[cfg(test)]
#[path = "indexer_republish/tests.rs"]
mod tests;
