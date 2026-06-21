//! ADR-0063 Lane A — producer-side row-delta tracker + the Lane B interface.
//!
//! [`RefRowDeltaTracker`] is the kernel-side producer: it remembers the per-key
//! rev it last emitted to a host and, given a [`RefRowRevSource`] (Lane B's
//! resolver), builds either a steady-state incremental batch (only rows whose
//! per-key rev advanced + explicit `Cleared` rows for keys that went absent) or
//! a full baseline batch (every live row as `Changed`).
//!
//! ## Lane B interface (`RefRowRevSource`)
//!
//! Lane B owns the unified `RefResolver`: the per-key rev map and the resolved
//! typed payloads keyed by `(namespace, key)`. Lane A consumes EXACTLY this
//! trait — it never reimplements resolution. The three methods are the complete
//! surface Lane A needs:
//!
//! - `ref_row_rev(ns, key)` — the per-key monotonic rev (the counter Lane B
//!   bumps when a key's resolved value changes; ADR-0063 §"per-key reactivity").
//! - `ref_row_keys(ns)` — the live key set, for baseline reconstruction and for
//!   detecting keys that went absent (→ explicit `Cleared`).
//! - `ref_row_payload(ns, key)` — the namespace's typed resolved bytes for a
//!   live key (the profile card / event embed). `None` only for a key that is
//!   not currently live.
//!
//! Until Lane B lands on the integration branch the trait is satisfied by
//! [`MapRowRevSource`] (in-memory stub). When Lane B lands, its resolver impls
//! `RefRowRevSource` and the stub is deleted. No reimplementation of resolution
//! lives here.

use super::rowdelta::{RefRow, RefRowDeltaBatch};
use std::collections::{BTreeMap, HashMap};

/// Lane B's per-key rev source. See the module doc-comment for the contract.
pub trait RefRowRevSource {
    /// Per-key monotonic revision for `(namespace, key)`. 0 for an unknown key.
    fn ref_row_rev(&self, namespace: &str, key: &str) -> u64;
    /// The currently-live key set for `namespace`.
    fn ref_row_keys(&self, namespace: &str) -> Vec<String>;
    /// The typed resolved payload bytes for a live `(namespace, key)`, or `None`
    /// when the key is not live.
    fn ref_row_payload(&self, namespace: &str, key: &str) -> Option<Vec<u8>>;
}

/// Kernel-side producer: tracks the per-key rev last emitted to one host and
/// builds incremental / baseline batches under the ADR-0063 invariants.
///
/// One tracker instance corresponds to one host's incremental-apply contract
/// (per-attach, ADR-0055 HA-2). A `reset` (session/epoch re-baseline) clears the
/// last-emitted map so the next build is a full baseline.
#[derive(Debug, Default)]
pub struct RefRowDeltaTracker {
    /// `namespace -> (key -> last-emitted rev)`. A key present here was emitted
    /// to the host at that rev; absence means "never emitted".
    last_emitted: HashMap<String, HashMap<String, u64>>,
}

impl RefRowDeltaTracker {
    /// Fresh tracker (next build for any namespace is implicitly a baseline of
    /// whatever is live, since nothing has been emitted yet).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear ALL last-emitted state (every namespace). Called on a session /
    /// epoch re-baseline so the next [`Self::build_baseline`] re-seeds the host
    /// from scratch (ADR-0063 invariant #3 / ADR-0055 D4).
    pub fn reset(&mut self) {
        self.last_emitted.clear();
    }

    /// Build a FULL baseline batch for `namespace`: every live row as `Changed`,
    /// `baseline = true`. Records the emitted revs (so a subsequent
    /// [`Self::build_incremental`] is correct).
    ///
    /// A live key whose payload the source cannot produce is skipped (it is not
    /// truly live); this never emits a `Changed` row with empty bytes (which a
    /// host would reject as decode-before-commit failure).
    pub fn build_baseline(
        &mut self,
        namespace: &str,
        source: &dyn RefRowRevSource,
    ) -> RefRowDeltaBatch {
        let mut emitted: HashMap<String, u64> = HashMap::new();
        let mut rows = Vec::new();
        for key in sorted_keys(source, namespace) {
            let Some(payload) = source.ref_row_payload(namespace, &key) else {
                continue;
            };
            let rev = source.ref_row_rev(namespace, &key);
            rows.push(RefRow::changed(key.clone(), rev, payload));
            emitted.insert(key, rev);
        }
        self.last_emitted.insert(namespace.to_string(), emitted);
        RefRowDeltaBatch {
            namespace: namespace.to_string(),
            baseline: true,
            rows,
        }
    }

    /// Build a steady-state incremental batch for `namespace`:
    ///
    /// - a live key whose rev advanced past the last emitted rev (or that was
    ///   never emitted) → a `Changed` row carrying its current payload;
    /// - a previously-emitted key that is no longer live → an explicit
    ///   `Cleared` row (invariant #1: absence is NOT how a clear is signalled);
    /// - a live key whose rev did not advance → OMITTED (Unchanged).
    ///
    /// Records the new per-key state so the next build is correct.
    pub fn build_incremental(
        &mut self,
        namespace: &str,
        source: &dyn RefRowRevSource,
    ) -> RefRowDeltaBatch {
        let prev = self
            .last_emitted
            .get(namespace)
            .cloned()
            .unwrap_or_default();
        let live: BTreeMap<String, u64> = sorted_keys(source, namespace)
            .into_iter()
            .map(|k| {
                let rev = source.ref_row_rev(namespace, &k);
                (k, rev)
            })
            .collect();

        let mut rows = Vec::new();
        let mut emitted: HashMap<String, u64> = HashMap::new();

        // Changed / Unchanged for live keys.
        for (key, rev) in &live {
            let advanced = match prev.get(key) {
                None => true,             // never emitted → must send
                Some(last) => rev > last, // advanced since last emit
            };
            if advanced {
                if let Some(payload) = source.ref_row_payload(namespace, key) {
                    rows.push(RefRow::changed(key.clone(), *rev, payload));
                    emitted.insert(key.clone(), *rev);
                    continue;
                }
                // Live key with no payload: treat as not-yet-resolvable; keep
                // prior emitted rev (do not advance) so a later batch re-sends.
            }
            // Unchanged (or unresolved-this-tick): retain prior emitted rev.
            if let Some(last) = prev.get(key) {
                emitted.insert(key.clone(), *last);
            }
        }

        // Cleared for keys that were emitted but are no longer live.
        for (key, last) in &prev {
            if !live.contains_key(key) {
                rows.push(RefRow::cleared(key.clone(), *last));
                // dropped from `emitted` → host removes it
            }
        }

        self.last_emitted.insert(namespace.to_string(), emitted);
        RefRowDeltaBatch {
            namespace: namespace.to_string(),
            baseline: false,
            rows,
        }
    }
}

fn sorted_keys(source: &dyn RefRowRevSource, namespace: &str) -> Vec<String> {
    let mut keys = source.ref_row_keys(namespace);
    keys.sort();
    keys.dedup();
    keys
}

// ── Lane B stub ────────────────────────────────────────────────────────────────

/// In-memory [`RefRowRevSource`] stub standing in for Lane B's resolver until it
/// lands on the integration branch. Holds `(namespace, key) -> (rev, payload)`.
///
/// This is the EXACT shape Lane A needs from Lane B; it is a test/bench fixture,
/// never a production resolver. Delete when Lane B's `RefResolver` implements
/// `RefRowRevSource`.
#[derive(Debug, Default, Clone)]
pub struct MapRowRevSource {
    rows: HashMap<String, BTreeMap<String, (u64, Vec<u8>)>>,
}

impl MapRowRevSource {
    /// Empty source.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or update a row, bumping its per-key rev to `rev` and setting its
    /// payload. Models a resolve / ingest event for `(namespace, key)`.
    pub fn upsert(&mut self, namespace: &str, key: &str, rev: u64, payload: Vec<u8>) {
        self.rows
            .entry(namespace.to_string())
            .or_default()
            .insert(key.to_string(), (rev, payload));
    }

    /// Remove a row (models a `release_ref`). The next incremental build emits
    /// an explicit `Cleared` row for it.
    pub fn remove(&mut self, namespace: &str, key: &str) {
        if let Some(ns) = self.rows.get_mut(namespace) {
            ns.remove(key);
        }
    }
}

impl RefRowRevSource for MapRowRevSource {
    fn ref_row_rev(&self, namespace: &str, key: &str) -> u64 {
        self.rows
            .get(namespace)
            .and_then(|ns| ns.get(key))
            .map(|(rev, _)| *rev)
            .unwrap_or(0)
    }

    fn ref_row_keys(&self, namespace: &str) -> Vec<String> {
        self.rows
            .get(namespace)
            .map(|ns| ns.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn ref_row_payload(&self, namespace: &str, key: &str) -> Option<Vec<u8>> {
        self.rows
            .get(namespace)
            .and_then(|ns| ns.get(key))
            .map(|(_, payload)| payload.clone())
    }
}
