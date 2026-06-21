//! ADR-0063 Lane A — reference host-side per-key cache (the Rust model).
//!
//! [`RefRowCache`] is the canonical reference implementation of the row-keyed
//! host cache. The generated Swift (`ProjectionCache.generated.swift`) and
//! Kotlin (`ProjectionCache.kt`) per-key caches implement the SAME algorithm;
//! this Rust model is what the invariant property harness asserts
//! `incremental-applied == full-snapshot` against, so the three implementations
//! cannot silently diverge on the correctness-critical merge.
//!
//! The algorithm enforces the five ADR-0063 invariants at ROW grain:
//! 1. an absent row is Unchanged (retained), never Cleared;
//! 2. decode-before-commit: a `Changed` row commits only after its payload
//!    decodes; a malformed row leaves the prior cached row intact + latches
//!    `needs_resync` (D6, fail-closed);
//! 3. a `baseline` batch / session-or-epoch change reconstructs the full set;
//! 4. payloads are namespace-typed bytes;
//! 5. the cache is host-side read-model only — truth stays kernel-owned.

use super::rowdelta::{RefRowDeltaBatch, RefRowState};
use std::collections::{BTreeMap, HashMap, HashSet};

/// One cached row: the last committed per-key rev + raw typed payload bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedRow {
    rev: u64,
    payload: Vec<u8>,
}

/// Outcome of applying one batch: the rows that changed (committed or cleared)
/// and whether a decode-before-commit failure latched `needs_resync`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefRowApplyOutcome {
    /// Keys whose cached row changed this batch (committed `Changed` or removed
    /// `Cleared`). The host re-renders exactly these per-key observable slots.
    pub changed_keys: Vec<String>,
    /// True iff at least one `Changed` row failed decode-before-commit this
    /// batch (prior row retained; host is known-degraded until resync/baseline).
    pub decode_failed: bool,
}

/// The host-side per-namespace row cache.
#[derive(Debug, Default)]
pub struct RefRowCache {
    /// `namespace -> (key -> CachedRow)`.
    rows: HashMap<String, HashMap<String, CachedRow>>,
    applied_session: u64,
    applied_epoch: u64,
    /// False until the first batch is applied after a (re)baseline. UI gates on
    /// this (ADR-0055 D3-5).
    baselined: bool,
    /// Sticky: latches on any decode-before-commit failure. Drained by a resync
    /// (a follow-on rung); cleared on session/epoch re-baseline.
    needs_resync: bool,
}

impl RefRowCache {
    /// Fresh empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the cache has applied a baseline (UI-gating flag).
    #[must_use]
    pub fn baselined(&self) -> bool {
        self.baselined
    }

    /// Whether a decode-before-commit failure is latched.
    #[must_use]
    pub fn needs_resync(&self) -> bool {
        self.needs_resync
    }

    /// The full materialized state of a namespace: `key -> payload`. The harness
    /// asserts this equals the producer's ground-truth full snapshot.
    #[must_use]
    pub fn snapshot(&self, namespace: &str) -> BTreeMap<String, Vec<u8>> {
        self.rows
            .get(namespace)
            .map(|ns| {
                ns.iter()
                    .map(|(k, v)| (k.clone(), v.payload.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The cached payload for one key, or `None` if absent (the per-key
    /// observable accessor the generated cache exposes, e.g. `profile(pubkey)`).
    #[must_use]
    pub fn get(&self, namespace: &str, key: &str) -> Option<Vec<u8>> {
        self.rows
            .get(namespace)
            .and_then(|ns| ns.get(key))
            .map(|row| row.payload.clone())
    }

    /// Forcibly corrupt a cached row's payload (test helper for the
    /// epoch-resync-repairs invariant — models a host-side cache corruption).
    #[cfg(test)]
    pub(crate) fn corrupt_for_test(&mut self, namespace: &str, key: &str, payload: Vec<u8>) {
        if let Some(row) = self.rows.get_mut(namespace).and_then(|ns| ns.get_mut(key)) {
            row.payload = payload;
        }
    }

    /// Apply one keyed-projection batch under the frame's `session_id` / `epoch`.
    ///
    /// `decode_ok` is the decode-before-commit preflight: `(key, payload) ->
    /// bool`. A `Changed` row commits only when it returns `true`; on `false`
    /// the prior row is retained and `decode_failed` is latched (D6).
    ///
    /// D4: a changed `session_id` or `epoch` clears ALL namespaces and resets
    /// identity BEFORE any row is merged.
    ///
    /// A `baseline` batch is applied ATOMICALLY via scratch-then-commit: every
    /// required row is decoded into a scratch namespace map first and the prior
    /// namespace is replaced ONLY after the whole baseline decodes. If ANY
    /// required row fails decode-before-commit the prior cache is preserved
    /// untouched and `needs_resync` latches (D6, fail-closed) — a malformed
    /// baseline row can never drop or corrupt a prior cached slot.
    pub fn apply(
        &mut self,
        batch: &RefRowDeltaBatch,
        session_id: u64,
        epoch: u64,
        decode_ok: &dyn Fn(&str, &[u8]) -> bool,
    ) -> RefRowApplyOutcome {
        // D4 — mandatory full reset on session/epoch change, before any merge.
        if session_id != self.applied_session || epoch != self.applied_epoch {
            self.rows.clear();
            self.applied_session = session_id;
            self.applied_epoch = epoch;
            self.baselined = false;
            self.needs_resync = false;
        }

        if batch.baseline {
            return self.apply_baseline(batch, decode_ok);
        }
        self.apply_incremental(batch, decode_ok)
    }

    /// Scratch-then-commit baseline (invariant #3 + decode-before-commit on the
    /// whole batch). Decodes every `Changed` row into a fresh scratch map; only
    /// when ALL required rows decode does it atomically replace the namespace.
    /// On any decode failure the prior cache is preserved and resync latches.
    fn apply_baseline(
        &mut self,
        batch: &RefRowDeltaBatch,
        decode_ok: &dyn Fn(&str, &[u8]) -> bool,
    ) -> RefRowApplyOutcome {
        let mut scratch: HashMap<String, CachedRow> = HashMap::new();
        for row in &batch.rows {
            match row.state {
                // A baseline carries only live rows as `Changed`. A defensive
                // `Cleared` in a baseline means the key is simply absent from
                // the rebuilt set — drop it from scratch, never commit it.
                RefRowState::Cleared => {
                    scratch.remove(&row.key);
                }
                RefRowState::Changed => {
                    // Decode-before-commit: a single malformed row fails the
                    // WHOLE baseline closed — prior cache untouched, resync
                    // latched. We have not mutated `self.rows` yet.
                    if !decode_ok(&row.key, &row.payload) {
                        self.needs_resync = true;
                        return RefRowApplyOutcome {
                            changed_keys: Vec::new(),
                            decode_failed: true,
                        };
                    }
                    // Duplicate-key guard within one baseline: last-rev wins.
                    let insert = match scratch.get(&row.key) {
                        Some(existing) => row.rev > existing.rev,
                        None => true,
                    };
                    if insert {
                        scratch.insert(
                            row.key.clone(),
                            CachedRow {
                                rev: row.rev,
                                payload: row.payload.clone(),
                            },
                        );
                    }
                }
            }
        }

        // Atomic commit: diff prior vs scratch so the host re-renders exactly
        // the slots that changed (added / updated / dropped ghosts), then swap.
        let prior = self.rows.get(&batch.namespace);
        let mut changed: HashSet<String> = HashSet::new();
        for (key, row) in &scratch {
            match prior.and_then(|p| p.get(key)) {
                Some(prev) if prev.payload == row.payload => {}
                _ => {
                    changed.insert(key.clone());
                }
            }
        }
        if let Some(prior) = prior {
            for key in prior.keys() {
                if !scratch.contains_key(key) {
                    changed.insert(key.clone());
                }
            }
        }
        self.rows.insert(batch.namespace.clone(), scratch);
        self.baselined = true;
        let mut changed_keys: Vec<String> = changed.into_iter().collect();
        changed_keys.sort();
        RefRowApplyOutcome {
            changed_keys,
            decode_failed: false,
        }
    }

    /// Steady-state incremental merge. Per-row: an explicit `Cleared` removes
    /// the cached row only if its rev is NEWER than the cached row (rev-safe
    /// clear — a stale reordered clear can never delete a newer live row); a
    /// `Changed` row commits only if its rev advanced AND it decodes (otherwise
    /// the prior row is retained and resync latches).
    fn apply_incremental(
        &mut self,
        batch: &RefRowDeltaBatch,
        decode_ok: &dyn Fn(&str, &[u8]) -> bool,
    ) -> RefRowApplyOutcome {
        let ns = self.rows.entry(batch.namespace.clone()).or_default();
        let mut changed: HashSet<String> = HashSet::new();
        let mut decode_failed = false;

        for row in &batch.rows {
            match row.state {
                RefRowState::Cleared => {
                    // Rev-safe clear (reorder guard, BLOCKING-4): a clear carries
                    // Lane B's monotonic per-key release rev; it removes the row
                    // only when that rev is strictly newer than the cached rev.
                    // A stale reordered clear (older rev) is ignored, so it can
                    // never delete a newer cached row. A clear for an absent key
                    // is a no-op (the final baseline repairs any lost-clear gap).
                    if let Some(existing) = ns.get(&row.key) {
                        if row.rev > existing.rev {
                            ns.remove(&row.key);
                            changed.insert(row.key.clone());
                        }
                    }
                }
                RefRowState::Changed => {
                    // Reorder/duplicate guard: skip a row not newer than cached.
                    if let Some(existing) = ns.get(&row.key) {
                        if row.rev <= existing.rev {
                            continue;
                        }
                    }
                    // Decode-before-commit (D6, invariant #2): on failure keep
                    // the prior row, do NOT advance, latch resync.
                    if decode_ok(&row.key, &row.payload) {
                        ns.insert(
                            row.key.clone(),
                            CachedRow {
                                rev: row.rev,
                                payload: row.payload.clone(),
                            },
                        );
                        changed.insert(row.key.clone());
                    } else {
                        decode_failed = true;
                        self.needs_resync = true;
                    }
                }
            }
        }

        self.baselined = true;
        let mut changed_keys: Vec<String> = changed.into_iter().collect();
        changed_keys.sort();
        RefRowApplyOutcome {
            changed_keys,
            decode_failed,
        }
    }
}
