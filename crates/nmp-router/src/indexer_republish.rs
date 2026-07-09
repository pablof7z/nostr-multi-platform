//! Indexer-republish target-selection policy.
//!
//! The policy passively forwards accepted replaceable events from non-indexer
//! relays to configured indexer relays. `nmp-core` owns the generic observer
//! and pool send; this crate owns the routing/provenance decision.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::slots::IndexerRelaysSlot;
use nmp_core::substrate::{
    ExternalEventSinkPolicy, RawEventForwardPolicyContext, RawEventForwardTarget, SignedEventFrame,
    SinkDestination,
};
use nmp_core::KindFilter;
use nmp_network::role::RelayRole;
use nmp_store::{EventStore, RawEvent};

const DEDUP_CAPACITY: usize = 4096;

/// Runtime control handle for indexer republish policy state.
///
/// The handle is intentionally tiny and lock-free so composition roots can keep
/// it and toggle forwarding without rebuilding the policy registry or resetting
/// the kernel.
#[derive(Clone, Debug)]
pub struct IndexerRepublishPolicyHandle {
    enabled: Arc<AtomicBool>,
    forwarded: Arc<AtomicU64>,
}

impl IndexerRepublishPolicyHandle {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(enabled)),
            forwarded: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Count of event x indexer forwarding decisions emitted by this policy.
    ///
    /// The worker owns the actual relay-channel send; this counter stays on the
    /// policy handle because the indexer-republish concept owns the meaning of
    /// the forwarding decision.
    #[must_use]
    pub fn forwarded_count(&self) -> u64 {
        self.forwarded.load(Ordering::Relaxed)
    }

    fn inc_forwarded(&self) {
        self.forwarded.fetch_add(1, Ordering::Relaxed);
    }
}

/// Policy for forwarding replaceable events to indexer relays.
pub struct IndexerRepublishPolicy {
    handle: IndexerRepublishPolicyHandle,
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
    pub fn new(
        handle: IndexerRepublishPolicyHandle,
        context: RawEventForwardPolicyContext,
    ) -> Self {
        Self {
            handle,
            indexer_relays: context.indexer_relays,
            store: context.event_store,
            dedup: Mutex::new(DedupCache::new()),
        }
    }

    #[must_use]
    pub fn enabled(context: RawEventForwardPolicyContext) -> Self {
        Self::new(IndexerRepublishPolicyHandle::new(true), context)
    }

    #[must_use]
    pub fn replaceable_kind_filter() -> KindFilter {
        // Delegate to the canonical NIP-01 replaceable/addressable predicates
        // (`nmp-kinds` is the bit-for-bit SSOT) instead of hand-rolling the kind
        // ranges here. The previous hand-rolled {0,3} ∪ [10000,20000) ∪
        // [30000,40000) set silently dropped kind:41 (NIP-28 channel metadata),
        // the one special case `nostr::Kind::is_replaceable` adds beyond the
        // NIP-01 range text — see #3097.
        let kinds = (0u32..40_000)
            .filter(|kind| nmp_kinds::is_replaceable(*kind) || nmp_kinds::is_addressable(*kind));
        KindFilter::from_kinds(kinds)
    }

    #[must_use]
    pub fn forwarded_count(&self) -> u64 {
        self.handle.forwarded_count()
    }

    fn indexer_urls(&self) -> Vec<String> {
        self.indexer_relays
            .lock()
            .map(|guard| guard.as_slice().to_vec())
            .unwrap_or_default()
    }

    fn event_has_indexer_provenance(&self, raw: &RawEvent, indexer_urls: &[String]) -> bool {
        // raw here comes from query results (StoredEvent.raw) — id is verified hex.
        let Some(id_bytes) = raw.id_bytes() else {
            return false;
        };
        self.store
            .provenance_for(&id_bytes)
            .map(|entries| {
                entries
                    .iter()
                    .any(|entry| indexer_urls.contains(&entry.relay_url))
            })
            .unwrap_or(false)
    }
}

impl ExternalEventSinkPolicy for IndexerRepublishPolicy {
    fn kind_filter(&self) -> KindFilter {
        Self::replaceable_kind_filter()
    }

    fn destinations(&self, frame: &SignedEventFrame) -> Vec<SinkDestination> {
        let raw = frame.raw.as_ref();
        let source_relay_url = frame.source_relay.as_deref();

        if !self.handle.is_enabled() {
            tracing::debug!(
                target: "nmp.router.indexer_republish",
                event_id = %raw.id,
                kind = raw.kind,
                reason = "disabled",
                "skipping indexer republish"
            );
            return Vec::new();
        }

        if !raw.is_replaceable() && !raw.is_param_replaceable() {
            tracing::debug!(
                target: "nmp.router.indexer_republish",
                event_id = %raw.id,
                kind = raw.kind,
                reason = "not_replaceable",
                "skipping indexer republish"
            );
            return Vec::new();
        }

        let indexer_urls = self.indexer_urls();
        if indexer_urls.is_empty() {
            tracing::debug!(
                target: "nmp.router.indexer_republish",
                event_id = %raw.id,
                kind = raw.kind,
                reason = "no_indexers",
                "skipping indexer republish"
            );
            return Vec::new();
        }

        if let Some(source) = source_relay_url {
            if indexer_urls.iter().any(|url| url == source) {
                tracing::debug!(
                    target: "nmp.router.indexer_republish",
                    event_id = %raw.id,
                    kind = raw.kind,
                    source_relay = %source,
                    reason = "source_is_indexer",
                    "skipping indexer republish"
                );
                return Vec::new();
            }
        }

        if self.event_has_indexer_provenance(raw, &indexer_urls) {
            tracing::debug!(
                target: "nmp.router.indexer_republish",
                event_id = %raw.id,
                kind = raw.kind,
                reason = "indexer_provenance",
                "skipping indexer republish"
            );
            return Vec::new();
        }

        let mut targets = Vec::new();
        for target in indexer_urls {
            if source_relay_url.is_some_and(|source| source == target.as_str()) {
                tracing::debug!(
                    target: "nmp.router.indexer_republish",
                    event_id = %raw.id,
                    kind = raw.kind,
                    relay_url = %target,
                    reason = "source_target_match",
                    "skipping indexer republish target"
                );
                continue;
            }
            let key = (raw.id.clone(), target.clone());
            let should_forward = self
                .dedup
                .lock()
                .map(|mut guard| guard.insert(key))
                .unwrap_or(false);
            if should_forward {
                self.handle.inc_forwarded();
                tracing::debug!(
                    target: "nmp.router.indexer_republish",
                    event_id = %raw.id,
                    kind = raw.kind,
                    relay_url = %target,
                    "forwarding event to indexer relay"
                );
                targets.push(SinkDestination::Relay(RawEventForwardTarget::new(
                    target,
                    RelayRole::Indexer,
                )));
            } else {
                tracing::debug!(
                    target: "nmp.router.indexer_republish",
                    event_id = %raw.id,
                    kind = raw.kind,
                    relay_url = %target,
                    reason = "dedup_hit",
                    "skipping indexer republish target"
                );
            }
        }
        targets
    }
}

#[cfg(test)]
#[path = "indexer_republish/tests.rs"]
mod tests;
