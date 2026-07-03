//! The causal trail: two views over the same `WalletFact` stream.
//!
//! - [`WalletDeltaRing`] is a time-ordered, bounded, eviction-based view:
//!   "what sequence of events produced this shape?"
//! - [`WalletCauseIndex`] is an `O(current state)` per-atom view, keyed by
//!   the thing it explains (a token/nutzap event, a proof, or a saga
//!   correlation id): "why is *this* here?" It is populated from the same
//!   facts as the ring but never evicts an atom the wallet still holds, so a
//!   flood of unrelated traffic cannot erase the explanation for a token the
//!   user still has. The ring is a diagnostic log; the index is derived
//!   state kept alongside it, and it is never rebuilt by replaying the ring
//!   — see `crate::journal::ledger` for why.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use super::fact::{CorrelationId, ProofRef, WalletEventId, WalletFact};

#[derive(Clone, Debug)]
pub struct WalletTrailEntry {
    pub sequence: u64,
    pub fact: Arc<WalletFact>,
}

/// Bounded time-ordered ring of trail entries. Oldest entries are evicted
/// first once `capacity` is exceeded — this is a diagnostic surface, not a
/// rebuild authority (`WalletLedger::rebuild_from` never reads from it).
#[derive(Debug)]
pub struct WalletDeltaRing {
    capacity: usize,
    next_sequence: u64,
    entries: VecDeque<WalletTrailEntry>,
}

impl WalletDeltaRing {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_sequence: 0,
            entries: VecDeque::new(),
        }
    }

    pub fn push(&mut self, fact: Arc<WalletFact>) -> WalletTrailEntry {
        let entry = WalletTrailEntry {
            sequence: self.next_sequence,
            fact,
        };
        self.next_sequence += 1;
        self.entries.push_back(entry.clone());
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        entry
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &WalletTrailEntry> {
        self.entries.iter()
    }
}

/// Per-atom last-cause index: `token/nutzap event id`, `proof`, or
/// `correlation id` -> the last fact that touched it. `O(current state)`,
/// not `O(traffic)` — entries are only replaced, never evicted by capacity.
#[derive(Debug, Default)]
pub struct WalletCauseIndex {
    by_event: BTreeMap<WalletEventId, Arc<WalletFact>>,
    by_proof: BTreeMap<ProofRef, Arc<WalletFact>>,
    by_correlation: BTreeMap<CorrelationId, Arc<WalletFact>>,
}

impl WalletCauseIndex {
    pub fn record_event_cause(&mut self, event: WalletEventId, fact: Arc<WalletFact>) {
        self.by_event.insert(event, fact);
    }

    pub fn record_proof_cause(&mut self, proof: ProofRef, fact: Arc<WalletFact>) {
        self.by_proof.insert(proof, fact);
    }

    pub fn record_correlation_cause(&mut self, op: CorrelationId, fact: Arc<WalletFact>) {
        self.by_correlation.insert(op, fact);
    }

    #[must_use]
    pub fn last_event_cause(&self, event: &WalletEventId) -> Option<&WalletFact> {
        self.by_event.get(event).map(Arc::as_ref)
    }

    #[must_use]
    pub fn last_proof_cause(&self, proof: &ProofRef) -> Option<&WalletFact> {
        self.by_proof.get(proof).map(Arc::as_ref)
    }

    #[must_use]
    pub fn last_correlation_cause(&self, op: &CorrelationId) -> Option<&WalletFact> {
        self.by_correlation.get(op).map(Arc::as_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::fact::{MintUrl, Provenance, WalletUnit};

    fn token_added(id: &str) -> Arc<WalletFact> {
        Arc::new(WalletFact::TokenAdded {
            token_event: WalletEventId::new(id),
            mint: MintUrl::new("https://mint.example"),
            unit: WalletUnit::new("sat"),
            proofs: Vec::new(),
            via: Provenance::MintRollover,
        })
    }

    #[test]
    fn ring_evicts_oldest_entry_past_capacity() {
        let mut ring = WalletDeltaRing::with_capacity(2);
        ring.push(token_added("a"));
        ring.push(token_added("b"));
        ring.push(token_added("c"));

        assert_eq!(ring.len(), 2);
        let sequences: Vec<u64> = ring.iter().map(|entry| entry.sequence).collect();
        assert_eq!(sequences, vec![1, 2]);
    }

    #[test]
    fn cause_index_survives_ring_eviction() {
        let mut ring = WalletDeltaRing::with_capacity(1);
        let mut causes = WalletCauseIndex::default();

        let fact = token_added("kept");
        ring.push(Arc::clone(&fact));
        causes.record_event_cause(WalletEventId::new("kept"), Arc::clone(&fact));

        // Evict "kept" out of the ring with unrelated traffic.
        for idx in 0..10 {
            let flood = token_added(&format!("flood-{idx}"));
            ring.push(Arc::clone(&flood));
        }
        assert_eq!(ring.len(), 1);
        assert!(ring
            .iter()
            .all(|entry| !matches!(entry.fact.as_ref(), WalletFact::TokenAdded { token_event, .. } if token_event.as_str() == "kept")));

        // The cause index still knows why "kept" exists.
        assert!(causes
            .last_event_cause(&WalletEventId::new("kept"))
            .is_some());
    }
}
