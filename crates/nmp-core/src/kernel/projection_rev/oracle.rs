//! ADR-0070 Rung 1 — biconditional completeness oracle.
//!
//! `cfg(any(test, feature = "test-support"))` only. ZERO production cost.
//!
//! ## The oracle (codex #2/meta-b)
//!
//! After the single production encode, for each Tier-2 built-in, fingerprint the
//! EXACT host cache unit:
//!
//!   `hash(presence ⊕ rev ⊕ encoded typed payload ⊕ schema metadata)`
//!
//! Assert per key per emit:
//!
//!   `(rev_advanced || presence_changed) ⟺ cache_unit_changed`
//!
//! This is the biconditional completeness oracle: if the projection's logical
//! content changed (different payload bytes) the rev MUST advance; if the rev
//! advanced the payload MUST differ. A stale stamp (rev advances but payload
//! unchanged) wastes bandwidth. A missed stamp (payload changed but rev didn't
//! advance) is a correctness bug.
//!
//! The oracle reuses the post-`merge_builtin_typed_projections` encode already
//! produced by `make_update` — zero double-encode.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::update_envelope::TypedProjectionData;

use super::{ProjectionManifest, ProjectionPresence, ProjectionRevTracker};

/// Fingerprint of a single projection's host CACHE UNIT — the bytes the host
/// actually stores and reuses: `hash(key ++ payload_bytes)`.
///
/// Deliberately does NOT fold `rev` or `presence`: those are protocol metadata,
/// not cached content. The biconditional we check is
/// `payload_changed ⟺ (rev_advanced || presence_changed)`. If the fingerprint
/// included `rev`, a Changed-then-Unchanged sequence with identical payload would
/// spuriously register as "cache unit changed" (because the rev field differs)
/// and produce a false violation. Folding `key` keeps distinct keys from
/// colliding when both carry an empty payload.
fn fingerprint(key: &str, payload: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    payload.hash(&mut h);
    h.finish()
}

/// ADR-0070 (#1671 integration glue) — the two keyed row-delta carriers
/// (`refs.profile` / `refs.event`) are exempt from the biconditional oracle.
///
/// The oracle's model is a FULL-SNAPSHOT projection: its "cache unit" is the
/// whole encoded payload, and the biconditional is `payload_bytes_changed ⟺
/// rev_advanced`. A row-delta carrier deliberately violates that: its payload
/// is a per-tick DELTA (an empty NRRD batch on a no-change tick, a full baseline
/// after an epoch flip, a single changed row otherwise), so the bytes legitimately
/// differ tick-to-tick even when the source counter (and thus the rev) did not
/// advance — which the oracle would mis-read as a StaleStamp. Correctness for the
/// carrier is enforced instead by the real Lane A `incremental == full` cache
/// property test (the campaign's headline gate), not by this snapshot oracle.
fn is_ref_row_delta_key(key: &str) -> bool {
    key == "refs.profile" || key == "refs.event"
}

/// One oracle assertion result.
#[derive(Debug)]
pub struct OracleViolation {
    pub key: &'static str,
    pub kind: OracleViolationKind,
}

#[derive(Debug, Eq, PartialEq)]
pub enum OracleViolationKind {
    /// The cache unit changed but the rev did NOT advance. Correctness bug —
    /// this is the direction Rung 1 enforces (a missed stamp = silent stale UI).
    StaleStamp,
}

/// Check the biconditional oracle for all Tier-2 built-ins.
///
/// `prev_fingerprints`: fingerprints from the PREVIOUS emit (or empty on first
/// tick). `manifest`: the manifest AFTER the current bump. `typed`: the typed
/// projections emitted THIS tick (post-`merge_builtin_typed_projections`).
///
/// Returns a list of violations. An empty list means the oracle passes.
pub fn check_oracle(
    prev_fingerprints: &std::collections::HashMap<&'static str, u64>,
    manifest: &ProjectionManifest,
    typed: &[TypedProjectionData],
) -> Vec<OracleViolation> {
    let mut violations = Vec::new();
    for state in &manifest.states {
        let key = state.key;
        // ADR-0070: row-delta carriers carry per-tick deltas, not a full
        // snapshot — the oracle's payload-bytes-vs-rev biconditional does not
        // model them. Skip (see `is_ref_row_delta_key`).
        if is_ref_row_delta_key(key) {
            continue;
        }
        // Find the typed payload for this key. A key with no typed sidecar entry
        // this tick (e.g. a drain key on a no-settlement tick) fingerprints over
        // an empty payload — the host's cache unit for it is "absent".
        let payload: &[u8] = typed
            .iter()
            .find(|t| t.key == key)
            .map(|t| t.payload.as_slice())
            .unwrap_or(&[]);

        let current_fp = fingerprint(key, payload);

        // First observation of a key is a BASELINE, not a change — the host has
        // no prior cache unit to be stale against. Only compare once we have a
        // previous fingerprint. (Note: `note_drain_emit`/`record_tick` and the
        // rev counters all start at 0, and the payload-content fingerprint folds
        // `key`, so a genuine 0 collision is not possible across distinct keys.)
        let Some(&prev_fp) = prev_fingerprints.get(key) else {
            continue;
        };

        let cache_unit_changed = current_fp != prev_fp;
        let rev_or_presence_advanced = state.presence == ProjectionPresence::Changed
            || state.presence == ProjectionPresence::Cleared;

        // Biconditional, "stale stamp" direction: the host's cache unit changed
        // (different bytes the host would have to re-cache) but the rev did NOT
        // advance and presence is Unchanged — so Rung 3 would suppress the frame
        // and the host would serve a stale projection. This is the correctness
        // bug F1/F2/F4/F5 each produce when a write chokepoint forgets its bump.
        //
        // The "spurious bump" direction (rev advanced but bytes identical) is a
        // bandwidth waste, not a correctness violation; Rung 3 will tighten it.
        if cache_unit_changed && !rev_or_presence_advanced {
            violations.push(OracleViolation {
                key,
                kind: OracleViolationKind::StaleStamp,
            });
        }
    }
    violations
}

/// Per-tick oracle state that a test harness holds across ticks.
#[derive(Default)]
pub struct OracleState {
    pub prev_fingerprints: std::collections::HashMap<&'static str, u64>,
    /// The manifest produced by the MOST RECENT emit, captured BEFORE
    /// `record_tick` advanced the tracker's last-emit baseline and cleared the
    /// drain `pending_presence` overrides. Tests read this to assert the
    /// presence (`Changed` / `Cleared` / `Unchanged`) the emit actually carried —
    /// reading the live manifest post-emit would always show `Unchanged`.
    pub last_emit_manifest: Option<ProjectionManifest>,
}

impl OracleState {
    /// Update the stored fingerprints for the next tick.
    pub fn record_tick(
        &mut self,
        manifest: &ProjectionManifest,
        typed: &[TypedProjectionData],
        tracker: &mut ProjectionRevTracker,
    ) {
        for state in &manifest.states {
            let key = state.key;
            // ADR-0070: skip the row-delta carriers — they are exempt from the
            // oracle (see `is_ref_row_delta_key`), so we neither fingerprint nor
            // advance a last-emit baseline for them here. Their last-emitted rev
            // is advanced by `record_emitted_for_manifest` on the production path;
            // under the oracle the manifest presence is recomputed each tick from
            // the source counter, which is sufficient (the carrier owns its own
            // per-row last-emitted state in `RefRowDeltaTracker`).
            if is_ref_row_delta_key(key) {
                tracker.record_emitted(key);
                continue;
            }
            let payload: &[u8] = typed
                .iter()
                .find(|t| t.key == key)
                .map(|t| t.payload.as_slice())
                .unwrap_or(&[]);
            let fp = fingerprint(key, payload);
            self.prev_fingerprints.insert(key, fp);
            // Record the emit so the tracker knows this key was served.
            tracker.record_emitted(key);
        }
        // Stash the manifest AS EMITTED (presence overrides still applied) so a
        // test can inspect the real per-key presence this tick carried.
        self.last_emit_manifest = Some(manifest.clone());
    }
}
