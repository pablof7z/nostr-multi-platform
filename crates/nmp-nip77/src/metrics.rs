//! Per-(filter, relay) sync counters.
//!
//! Two integers per pair, both monotonically increasing:
//!
//! | Counter | Increment trigger |
//! |---|---|
//! | `bytes_on_wire_via_neg` | every byte we send or receive inside a `NEG-MSG` frame |
//! | `bytes_saved_vs_req` | (REQ-baseline-bytes − negentropy-bytes) for the same `(filter, relay)` pair, clamped to ≥ 0 |
//!
//! The diagnostic surface in [`MetricsSnapshot`] is plain `serde` so the
//! ADR-0007 diagnostics bridge can ship it through `AppState.debug` without
//! a per-counter FFI wrapper.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

/// Composite key — `(filter_hash, relay_url)` is exactly what the watermark
/// table uses, so the diagnostic surface lines up with the sync target the
/// planner already knows about.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct RelayFilterKey {
    pub filter_hash_hex: String,
    pub relay_url: String,
}

impl RelayFilterKey {
    pub fn new(filter_hash: [u8; 32], relay_url: impl Into<String>) -> Self {
        Self {
            filter_hash_hex: hex32(&filter_hash),
            relay_url: relay_url.into(),
        }
    }
}

/// Render a serializable snapshot for diagnostics.
#[derive(Clone, Debug, Serialize)]
pub struct MetricsSnapshot {
    pub per_pair: HashMap<RelayFilterKey, PairCounters>,
    pub totals: TotalCounters,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct PairCounters {
    pub bytes_on_wire_via_neg: u64,
    pub bytes_saved_vs_req: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct TotalCounters {
    pub bytes_on_wire_via_neg: u64,
    pub bytes_saved_vs_req: u64,
}

/// Thread-safe in-memory counters.
pub struct SyncMetrics {
    inner: Mutex<HashMap<RelayFilterKey, PairCounters>>,
}

impl SyncMetrics {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Add `n` bytes to the negentropy-on-wire counter for this pair.
    pub fn record_neg_bytes(&self, key: &RelayFilterKey, n: u64) {
        let mut guard = self.inner.lock().expect("metrics lock poisoned");
        let entry = guard.entry(key.clone()).or_default();
        entry.bytes_on_wire_via_neg = entry.bytes_on_wire_via_neg.saturating_add(n);
    }

    /// Record a (REQ-baseline, neg-actual) pair, attributing the saving (or
    /// the regression — clamped to 0) to this pair.
    pub fn record_savings(&self, key: &RelayFilterKey, req_baseline: u64, neg_actual: u64) {
        let saving = req_baseline.saturating_sub(neg_actual);
        let mut guard = self.inner.lock().expect("metrics lock poisoned");
        let entry = guard.entry(key.clone()).or_default();
        entry.bytes_saved_vs_req = entry.bytes_saved_vs_req.saturating_add(saving);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        let guard = self.inner.lock().expect("metrics lock poisoned");
        let per_pair = guard.clone();
        let mut totals = TotalCounters::default();
        for c in per_pair.values() {
            totals.bytes_on_wire_via_neg = totals
                .bytes_on_wire_via_neg
                .saturating_add(c.bytes_on_wire_via_neg);
            totals.bytes_saved_vs_req = totals
                .bytes_saved_vs_req
                .saturating_add(c.bytes_saved_vs_req);
        }
        MetricsSnapshot { per_pair, totals }
    }
}

impl Default for SyncMetrics {
    fn default() -> Self {
        Self::new()
    }
}

fn hex32(bytes: &[u8; 32]) -> String {
    static HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in bytes.iter() {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> RelayFilterKey {
        RelayFilterKey::new([0x11; 32], "wss://r/")
    }

    #[test]
    fn neg_bytes_accumulate() {
        let m = SyncMetrics::new();
        m.record_neg_bytes(&key(), 100);
        m.record_neg_bytes(&key(), 50);
        let snap = m.snapshot();
        assert_eq!(snap.per_pair[&key()].bytes_on_wire_via_neg, 150);
        assert_eq!(snap.totals.bytes_on_wire_via_neg, 150);
    }

    #[test]
    fn savings_clamp_on_regression() {
        let m = SyncMetrics::new();
        m.record_savings(&key(), 10, 100); // neg cost more than REQ baseline
        let snap = m.snapshot();
        assert_eq!(snap.per_pair[&key()].bytes_saved_vs_req, 0);
    }

    #[test]
    fn savings_are_attributed_correctly() {
        let m = SyncMetrics::new();
        m.record_savings(&key(), 1000, 50);
        let snap = m.snapshot();
        assert_eq!(snap.per_pair[&key()].bytes_saved_vs_req, 950);
        assert_eq!(snap.totals.bytes_saved_vs_req, 950);
    }

    #[test]
    fn filter_hash_hex_is_lowercase() {
        let k = RelayFilterKey::new([0xab; 32], "wss://r/");
        assert!(k.filter_hash_hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(k.filter_hash_hex.len(), 64);
    }
}
