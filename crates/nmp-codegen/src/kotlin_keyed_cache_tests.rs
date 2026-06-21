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
    assert!(out.contains("if (batch.baseline)"));
    assert!(out.contains("RefRowState.Cleared"));
    assert!(out.contains("ns.remove(key)"));
    assert!(out.contains("needsResync = true"));
    assert!(out.contains("incomingRev <= cached.rev"));
    assert!(out.contains("notifyRowChange"));
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
