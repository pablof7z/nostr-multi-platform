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
fn emits_namespace_routing() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        assert!(out.contains(&format!("{:?} -> {:?}", e.projection_key, e.namespace)));
    }
}

/// ADR-0063 Lane C (#1671) BLOCKING/HIGH codex fix: Android must NOT expose a
/// public RAW per-namespace refs accessor. Until the `flatc --kotlin` row
/// readers (`nmp.kernel.ProfileSnapshot` / `ClaimedEventsSnapshot`) ship and the
/// TYPED accessor can be emitted (Lane G), the generator emits NO public
/// per-namespace surface: the cached bytes are reachable only via the `internal
/// payload(...)` merge primitive. This guards against re-introducing the
/// dishonest raw `ByteArray?` accessor invariant #4 forbids.
#[test]
fn emits_no_public_raw_per_namespace_accessor() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        // The old dishonest raw accessor must be GONE for every namespace.
        assert!(
            !out.contains(&format!(
                "fun {}(key: String): ByteArray? = payload({:?}, key)",
                e.accessor, e.projection_key
            )),
            "public raw ByteArray? accessor for {} must be removed (invariant #4)",
            e.accessor
        );
        // No public per-namespace refs accessor of ANY return shape yet — the
        // typed kotlin accessor lands in Lane G. (A TYPED accessor, once present,
        // would be `fun <accessor>(key: String): <DomainType>?` WITHOUT the
        // `internal` prefix; this asserts neither a public typed NOR a public raw
        // per-namespace accessor exists today.)
        assert!(
            !out.contains(&format!("\n    fun {}(key: String)", e.accessor)),
            "no public per-namespace accessor for {} until Lane G typed readers land",
            e.accessor
        );
    }
    // The row bytes stay reachable through the INTERNAL merge primitive.
    assert!(
        out.contains("internal fun payload(projectionKey: String, rowKey: String): ByteArray?"),
        "internal payload(...) merge primitive must remain (non-public)"
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
