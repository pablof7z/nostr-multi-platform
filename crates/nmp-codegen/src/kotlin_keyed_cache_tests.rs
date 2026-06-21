//! Structural unit tests for the generated Kotlin `KeyedRefCache` (ADR-0063).
//! The Kotlin compile + JUnit run is the CI harness's gate
//! (`android/app/src/test/java/org/nmp/android/KeyedRefCacheTest.kt`); these
//! Rust tests guard the generator's emitted shape.

use super::render_kotlin_keyed_ref_cache;
use crate::swift_projections_registry::KEYED_PROJECTIONS;

fn rendered() -> String {
    render_kotlin_keyed_ref_cache(KEYED_PROJECTIONS)
}

#[test]
fn emits_namespace_routing_and_accessors() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        assert!(out.contains(&format!("{:?} -> {:?}", e.projection_key, e.namespace)));
        assert!(out.contains(&format!(
            "fun {}(key: String): ByteArray? = payload({:?}, key)",
            e.accessor, e.projection_key
        )));
    }
}

#[test]
fn enforces_the_five_invariants() {
    let out = rendered();
    assert!(out.contains("if (sessionId != appliedSession || snapshotEpoch != appliedEpoch)"));
    assert!(out.contains("val isBaseline = batch.baseline"));
    assert!(out.contains("RefRowState.Cleared"));
    assert!(out.contains("ns.remove(row.key)"));
    assert!(out.contains("needsResync = true"));
    assert!(out.contains("row.rev <= cached.rev"));
    assert!(out.contains("notifyRowChange"));
}

/// BLOCKING-1/2/3/4 hardening: the generator must emit the fail-closed CHECKED
/// decode, the scratch-then-commit baseline, the decode seam, and rev-safe clears.
#[test]
fn emits_failclosed_and_revsafe_hardening() {
    let out = rendered();
    // BLOCKING-2: NRRD file-id verification + try/catch fail-closed.
    assert!(out.contains("RefRowDeltaBatch.RefRowDeltaBatchBufferHasIdentifier(bb)"));
    assert!(out.contains("} catch (e: Exception) {"));
    // BLOCKING-1: scratch-then-commit baseline.
    assert!(out.contains("private fun applyBaseline("));
    assert!(out.contains("val scratch = HashMap<String, RefRowCacheEntry>()"));
    // BLOCKING-3: per-namespace decode-before-commit seam.
    assert!(out.contains("var rowDecoder: (String, ByteArray) -> Boolean"));
    // BLOCKING-4: rev-safe clear (a clear removes only when strictly newer).
    assert!(out.contains("if (cached != null && row.rev > cached.rev)"));
}

#[test]
fn decodes_the_row_delta_batch_payload() {
    let out = rendered();
    assert!(out.contains("RefRowDeltaBatch.getRootAsRefRowDeltaBatch(bb)"));
    assert!(out.contains("for (i in 0 until batch.rowsLength)"));
}

#[test]
fn is_marked_generated() {
    assert!(rendered().contains("THIS FILE IS GENERATED. DO NOT EDIT BY HAND."));
}
