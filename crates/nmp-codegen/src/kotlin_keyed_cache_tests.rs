//! Structural unit tests for the generated Kotlin `KeyedRefCache` (ADR-0063).
//! The Kotlin compile + JUnit run is the CI harness's gate
//! (`apps/chirp/android/app/src/test/java/org/nmp/android/KeyedRefCacheTest.kt`); these
//! Rust tests guard the generator's emitted shape.

use super::render_kotlin_keyed_ref_cache;
use crate::swift_projections_registry::KEYED_PROJECTIONS;

fn rendered() -> String {
    render_kotlin_keyed_ref_cache(KEYED_PROJECTIONS)
}

#[test]
fn emits_namespace_routing() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        assert!(out.contains(&format!("{:?} -> {:?}", e.projection_key, e.namespace)));
    }
}

/// ADR-0063 Lane G (#1671): the public per-namespace surface is the TYPED
/// accessor (`profile(key) -> ProfileCard?` / `event(key) -> ClaimedEventDto?`),
/// NEVER a dishonest raw `ByteArray?` accessor (invariant #4). The accessor
/// decodes the cached row bytes through the namespace's typed Kotlin reader; the
/// raw bytes stay reachable only via the `internal payload(...)` merge primitive.
/// This mirrors the Swift typed twin exactly.
#[test]
fn emits_typed_per_namespace_accessor_and_no_raw_surface() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        let kotlin = e
            .row_payload
            .kotlin
            .as_ref()
            .expect("Lane G: every keyed projection carries a Kotlin typed descriptor");
        // The old dishonest raw accessor must be GONE for every namespace.
        assert!(
            !out.contains(&format!(
                "fun {}(key: String): ByteArray?",
                e.accessor
            )),
            "raw ByteArray? accessor for {} must NOT exist (invariant #4)",
            e.accessor
        );
        // The TYPED accessor MUST exist: `fun <accessor>(key: String): <Domain>?`.
        assert!(
            out.contains(&format!(
                "    fun {}(key: String): {}? {{",
                e.accessor, kotlin.domain_type
            )),
            "typed accessor `{}(key): {}?` must be emitted (Lane G)",
            e.accessor,
            kotlin.domain_type
        );
        // It routes through the per-namespace typed decode helper + the glue.
        assert!(
            out.contains(&format!("KeyedRefDecoders.{}(reader)", kotlin.glue)),
            "typed decoder for {} must call KeyedRefDecoders.{}",
            e.accessor,
            kotlin.glue
        );
    }
    // The row bytes stay reachable through the INTERNAL merge primitive.
    assert!(
        out.contains("internal fun payload(projectionKey: String, rowKey: String): ByteArray?"),
        "internal payload(...) merge primitive must remain (non-public)"
    );
    // Decode-before-commit seam installed at construction (Swift `init` twin).
    assert!(
        out.contains("installTypedRowDecoder()"),
        "init must install the typed decode-before-commit seam"
    );
}

#[test]
fn enforces_the_five_invariants() {
    let out = rendered();
    assert!(out.contains("val identityChanged = sessionId != appliedSession || snapshotEpoch != appliedEpoch"));
    assert!(out.contains("isBaseline = batch.baseline"));
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

/// BLOCKING-1/2/3 fail-closed: the generator must emit (1) a DEFERRED identity
/// reset (no eager `rows.clear()` before decode), (2) whole-batch rejection on a
/// missing key, (3) whole-batch rejection on an out-of-range state discriminant.
#[test]
fn emits_failclosed_missing_key_and_bad_state_and_deferred_reset() {
    let out = rendered();
    // BLOCKING-2: a row with no key rejects the WHOLE batch (no silent skip).
    assert!(out.contains("if (key == null) {"));
    assert!(out.contains("rejecting whole batch"));
    // The old fail-open `row.key ?: continue` row-skip must be GONE.
    assert!(
        !out.contains("val key = row.key ?: continue"),
        "row-skipping on missing key must be removed (fail-closed)"
    );
    // BLOCKING-3: an out-of-range state discriminant rejects the whole batch.
    assert!(
        out.contains("if (state > RefRowState.Cleared) {"),
        "unknown state discriminant must reject the whole batch (not coerce to Changed)"
    );
    // BLOCKING-1: identity reset is DEFERRED — no eager full clear at the top of
    // merge; the drop happens only after a valid baseline decode.
    assert!(
        !out.contains("rows.clear()\n            appliedSession = sessionId"),
        "identity reset must be deferred until after a valid baseline decode"
    );
    assert!(
        out.contains("val identityChanged = sessionId != appliedSession || snapshotEpoch != appliedEpoch"),
        "merge must compute identityChanged without clearing the cache"
    );
    // The deferred reset drops other projections only on a successful baseline.
    assert!(out.contains("rows.keys.retainAll { it == projectionKey }"));
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
