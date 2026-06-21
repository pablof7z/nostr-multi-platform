//! Structural unit tests for the generated Swift `KeyedRefCache` (ADR-0063).
//!
//! These assert the generator emits the correctness-critical constructs (the
//! five invariants) and one accessor per keyed namespace. The Swift COMPILE +
//! XCTest run is the device/CI harness's gate
//! (`ios/Chirp/ChirpTests/KeyedRefCacheTests.swift`); these Rust tests guard the
//! generator so the emitted shape cannot silently regress.

use super::render_keyed_ref_cache;
use crate::swift_projections_registry::KEYED_PROJECTIONS;

fn rendered() -> String {
    render_keyed_ref_cache(KEYED_PROJECTIONS)
}

#[test]
fn emits_namespace_routing_for_every_keyed_projection() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        assert!(
            out.contains(&format!("case {:?}: return {:?}", e.projection_key, e.namespace)),
            "missing routing for {}",
            e.projection_key
        );
    }
}

#[test]
fn emits_one_accessor_per_namespace() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        assert!(
            out.contains(&format!(
                "func {}(_ key: String) -> Data? {{ payload(projectionKey: {:?}, rowKey: key) }}",
                e.accessor, e.projection_key
            )),
            "missing accessor {}",
            e.accessor
        );
    }
}

#[test]
fn enforces_the_five_invariants() {
    let out = rendered();
    // Invariant #3: D4 session/epoch reset + baseline rebuild.
    assert!(out.contains("if sessionId != appliedSession || snapshotEpoch != appliedEpoch"));
    assert!(out.contains("if batch.baseline {"));
    // Invariant #1: absent row is never cleared — only an explicit Cleared row
    // removes (and it removes; absence is a no-op because omitted rows are not
    // iterated at all).
    assert!(out.contains("kRefRowStateCleared"));
    assert!(out.contains("ns.removeValue(forKey: key)"));
    // Invariant #2: decode-before-commit (empty payload keeps prior + resync).
    assert!(out.contains("needsResync = true"));
    assert!(out.contains("if bytes.isEmpty {"));
    // Reorder guard.
    assert!(out.contains("incomingRev <= cached.rev"));
    // Per-key observable publisher (one re-render per changed key).
    assert!(out.contains("PassthroughSubject<KeyedRowChange, Never>"));
    assert!(out.contains("rowChanged.send"));
}

#[test]
fn decodes_the_row_delta_batch_payload() {
    let out = rendered();
    assert!(out.contains("nmp_refs_RefRowDeltaBatch = getRoot(byteBuffer: &buffer)"));
    assert!(out.contains("for row in batch.rows"));
}

#[test]
fn is_marked_generated() {
    assert!(rendered().contains("THIS FILE IS GENERATED. DO NOT EDIT BY HAND."));
}
