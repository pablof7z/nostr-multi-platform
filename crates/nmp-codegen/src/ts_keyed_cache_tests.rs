//! Structural unit tests for the generated TypeScript `KeyedRefCache`
//! (ADR-0063 twin, #2722). The vitest run over the emitted
//! `keyedRefCache.generated.ts` is the CI harness's runtime gate
//! (`web/packages/runtime-web/src/keyedRefCache.generated.test.ts`); these
//! Rust tests guard the generator's emitted shape, mirroring
//! `kotlin_keyed_cache_tests.rs` / `swift_keyed_cache_tests.rs`.

use super::render_ts_keyed_ref_cache;
use crate::swift_projections_registry::KEYED_PROJECTIONS;

fn rendered() -> String {
    render_ts_keyed_ref_cache(KEYED_PROJECTIONS)
}

#[test]
fn is_marked_generated() {
    assert!(rendered().contains("THIS FILE IS GENERATED. DO NOT EDIT BY HAND."));
}

#[test]
fn keys_the_row_cache_by_projection_key_not_wire_namespace() {
    let out = rendered();
    assert!(out.contains("private rows = new Map<string, Map<string, CachedRow>>();"));
    assert!(out.contains("this.rows.set(projectionKey, scratch);"));
    assert!(out.contains("this.rows.set(projectionKey, ns);"));
}

/// Invariant #4: the public per-namespace surface is the TYPED accessor
/// (`profile(key) -> ProfileWire | undefined`), never a dishonest raw
/// `Uint8Array` accessor for a namespace that HAS a typed decoder.
#[test]
fn emits_typed_accessor_and_full_snapshot_for_every_ts_entry() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        let Some(ts) = e.row_payload.ts.as_ref() else {
            continue;
        };
        assert!(
            out.contains(&format!(
                "{}(key: string): {} | undefined {{",
                e.accessor, ts.domain_type
            )),
            "typed accessor `{}(key): {} | undefined` must be emitted",
            e.accessor,
            ts.domain_type
        );
        assert!(
            out.contains(&format!(
                "{}s(): Map<string, {}> {{",
                e.accessor, ts.domain_type
            )),
            "full-snapshot accessor `{}s(): Map<string, {}>` must be emitted",
            e.accessor,
            ts.domain_type
        );
        assert!(
            out.contains(&format!("return {}(reader);", ts.glue)),
            "typed decoder for {} must call the {} glue",
            e.accessor,
            ts.glue
        );
    }
}

#[test]
fn skips_typed_accessor_for_entries_with_no_ts_descriptor() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        if e.row_payload.ts.is_some() {
            continue;
        }
        assert!(
            !out.contains(&format!("{}(key: string):", e.accessor)),
            "no typed accessor should be emitted for {} (ts: None)",
            e.accessor
        );
    }
}

#[test]
fn enforces_the_five_invariants() {
    let out = rendered();
    assert!(out.contains(
        "const identityChanged = sessionId !== this.appliedSession || snapshotEpoch !== this.appliedEpoch;"
    ));
    assert!(out.contains("batch.baseline"));
    assert!(out.contains("row.state === \"cleared\""));
    assert!(out.contains("ns.delete(row.key)"));
    assert!(out.contains("this.needsResyncFlag = true"));
    assert!(out.contains("existing.rev"));
}

#[test]
fn decodes_the_row_delta_batch_payload_fail_closed() {
    let out = rendered();
    assert!(out.contains("RefRowDeltaBatchFb.bufferHasIdentifier(bb)"));
    assert!(out.contains("RefRowDeltaBatchFb.getRootAsRefRowDeltaBatch(bb)"));
    assert!(out.contains("if (key === null) return undefined;"));
    assert!(out.contains("if (!batch) {\n      this.needsResyncFlag = true;\n      return EMPTY_OUTCOME();\n    }"));
}

#[test]
fn baseline_is_scratch_then_commit() {
    let out = rendered();
    assert!(out.contains("private applyBaseline("));
    assert!(out.contains("const scratch = new Map<string, CachedRow>();"));
    assert!(out.contains("if (!existing || row.rev > existing.rev) {"));
}

#[test]
fn merge_gates_on_the_registered_projection_key_set() {
    let out = rendered();
    assert!(out.contains("private static isKeyedProjection(projectionKey: string): boolean {"));
    assert!(out.contains("if (!KeyedRefCache.isKeyedProjection(projectionKey)) {"));
    for e in KEYED_PROJECTIONS {
        assert!(out.contains(&format!("case {:?}:", e.projection_key)));
    }
}

#[test]
fn decode_before_commit_routes_by_projection_key() {
    let out = rendered();
    assert!(out.contains("private rowDecoder(projectionKey: string, payload: Uint8Array): boolean {"));
    assert!(out.contains("switch (projectionKey) {"));
    assert!(out.contains("return payload.length > 0;"));
}
