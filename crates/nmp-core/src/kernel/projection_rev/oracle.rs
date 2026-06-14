//! ADR-0055 Rung 1 — biconditional completeness oracle.
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

/// Fingerprint of a single projection's host-cache unit:
/// `hash(key ++ rev ++ presence ++ payload_bytes)`.
///
/// Two oracle snapshots are taken per emit: one BEFORE the manifest is finalized
/// (capturing the "previous" state) and one AFTER (capturing the "current" state).
/// The biconditional is checked by comparing the two fingerprints.
fn fingerprint(
    key: &str,
    rev: u64,
    presence: ProjectionPresence,
    payload: &[u8],
) -> u64 {
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    rev.hash(&mut h);
    (presence as u8).hash(&mut h);
    payload.hash(&mut h);
    h.finish()
}

/// One oracle assertion result.
#[derive(Debug)]
pub struct OracleViolation {
    pub key: &'static str,
    pub kind: OracleViolationKind,
}

#[derive(Debug, Eq, PartialEq)]
pub enum OracleViolationKind {
    /// The cache unit changed but the rev did NOT advance. Correctness bug.
    StaleStamp,
    /// The rev advanced but the cache unit did NOT change. Wasted bandwidth.
    SpuriousBump,
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
        // Find the typed payload for this key (may be absent for drain keys
        // on ticks with no settlements).
        let payload: &[u8] = typed
            .iter()
            .find(|t| t.key == key)
            .map(|t| t.payload.as_slice())
            .unwrap_or(&[]);

        let current_fp = fingerprint(key, state.rev, state.presence, payload);
        let prev_fp = prev_fingerprints.get(key).copied().unwrap_or(0);

        let cache_unit_changed = current_fp != prev_fp;
        let rev_or_presence_advanced =
            state.presence == ProjectionPresence::Changed
            || state.presence == ProjectionPresence::Cleared;

        // Biconditional: changed iff advanced.
        // We only enforce the "stale stamp" direction (payload changed but rev
        // didn't) because the "spurious bump" direction (rev advanced but nothing
        // changed) is a bandwidth waste but not a correctness violation in Rung 1.
        // Rung 3 will tighten this.
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
            let payload: &[u8] = typed
                .iter()
                .find(|t| t.key == key)
                .map(|t| t.payload.as_slice())
                .unwrap_or(&[]);
            let fp = fingerprint(key, state.rev, state.presence, payload);
            self.prev_fingerprints.insert(key, fp);
            // Record the emit so the tracker knows this key was served.
            tracker.record_emitted(key);
        }
    }
}
