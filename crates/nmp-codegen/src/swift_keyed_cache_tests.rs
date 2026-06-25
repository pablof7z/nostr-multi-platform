//! Structural unit tests for the generated Swift `KeyedRefCache` (ADR-0063).
//!
//! These assert the generator emits the correctness-critical constructs (the
//! five invariants) and one accessor per keyed namespace. The Swift COMPILE +
//! XCTest run is the device/CI harness's gate
//! (`apps/chirp/ios/ChirpTests/KeyedRefCacheTests.swift`); these Rust tests guard the
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
fn emits_one_typed_accessor_per_namespace() {
    let out = rendered();
    for e in KEYED_PROJECTIONS {
        // ADR-0063 Lane C: the accessor returns the TYPED domain type, NOT the
        // Lane-A raw `Data?` passthrough.
        assert!(
            out.contains(&format!(
                "func {}(_ key: String) -> {}? {{",
                e.accessor, e.row_payload.swift_domain_type
            )),
            "missing typed accessor {} -> {}?",
            e.accessor,
            e.row_payload.swift_domain_type
        );
        // The old raw-bytes passthrough form must be GONE for this accessor.
        assert!(
            !out.contains(&format!(
                "func {}(_ key: String) -> Data? {{ payload(projectionKey: {:?}, rowKey: key) }}",
                e.accessor, e.projection_key
            )),
            "the Lane-A raw `Data?` accessor for {} must be removed",
            e.accessor
        );
    }
}

/// ADR-0063 Lane C: the generator must emit a REAL typed decode-before-commit
/// seam (a CHECKED root decode of the ROW payload buffer into the namespace's
/// concrete reader + the hand-written glue), wired in at construction. This is
/// what replaces Lane A's `!payload.isEmpty` default and makes the host surface
/// typed rather than raw.
#[test]
fn emits_real_typed_decode_before_commit() {
    let out = rendered();
    // The init installs the typed decoder so the typed contract holds with no
    // caller wiring.
    assert!(out.contains("init() {\n        installTypedRowDecoder()"));
    assert!(out.contains("private func installTypedRowDecoder() {"));
    assert!(out.contains("rowDecoder = { [weak self] namespace, payload in"));
    for e in KEYED_PROJECTIONS {
        let rp = &e.row_payload;
        // A per-namespace CHECKED decode into the row reader, verifying the ROW
        // payload's OWN file id (KPRF / KCEV), NOT the NRRD batch id.
        assert!(
            out.contains(&format!(
                "reader = try getCheckedRoot(byteBuffer: &buffer, fileId: {}.id)",
                rp.swift_reader_type
            )),
            "missing CHECKED row decode for {}",
            e.projection_key
        );
        // The decode routes through the hand-written glue (reader -> domain).
        assert!(
            out.contains(&format!("return TypedProjectionGlue.{}(reader)", rp.swift_glue)),
            "missing glue call {} for {}",
            rp.swift_glue,
            e.projection_key
        );
        // The decode-before-commit seam routes the namespace to its typed helper.
        assert!(
            out.contains(&format!("case {:?}: return self.", e.namespace)),
            "missing namespace decode route for {}",
            e.namespace
        );
    }
}

#[test]
fn enforces_the_five_invariants() {
    let out = rendered();
    // Invariant #3: D4 session/epoch detection (deferred reset) + baseline rebuild.
    assert!(out.contains("let identityChanged = sessionId != appliedSession || snapshotEpoch != appliedEpoch"));
    assert!(out.contains("if batch.baseline {"));
    // Invariant #1: absent row is never cleared — only an explicit Cleared row
    // removes (and it removes; absence is a no-op because omitted rows are not
    // iterated at all).
    assert!(out.contains("kRefRowStateCleared"));
    assert!(out.contains("ns.removeValue(forKey: key)"));
    // Invariant #2: decode-before-commit via the typed seam (empty OR invalid
    // payload keeps prior + latches resync).
    assert!(out.contains("needsResync = true"));
    assert!(out.contains("bytes.isEmpty || !rowDecoder(namespace, bytes)"));
    // Reorder guard (Changed).
    assert!(out.contains("incomingRev <= cached.rev"));
    // Per-key observable publisher (one re-render per changed key).
    assert!(out.contains("PassthroughSubject<KeyedRowChange, Never>"));
    assert!(out.contains("rowChanged.send"));
}

/// BLOCKING-1/2/3/4 hardening: the generator must emit the fail-closed CHECKED
/// decode, the scratch-then-commit baseline, the decode seam, and rev-safe clears.
#[test]
fn emits_failclosed_and_revsafe_hardening() {
    let out = rendered();
    // BLOCKING-2: CHECKED root decode verifying the NRRD file id, in a do/catch
    // that fails closed (retains prior cache, latches resync).
    assert!(out.contains("try getCheckedRoot(byteBuffer: &buffer, fileId: nmp_refs_RefRowDeltaBatch.id)"));
    assert!(out.contains("} catch {"));
    // BLOCKING-1: scratch-then-commit baseline (atomic replace after all decode).
    assert!(out.contains("func applyBaseline("));
    assert!(out.contains("var scratch: [String: RefRowCacheEntry] = [:]"));
    // BLOCKING-3: per-namespace decode-before-commit seam.
    assert!(out.contains("var rowDecoder: (_ namespace: String, _ payload: Data) -> Bool"));
    // BLOCKING-4: rev-safe clear (a clear removes only when strictly newer).
    assert!(out.contains("if let cached = ns[key], row.rev > cached.rev {"));
}

/// BLOCKING-1/2/3 fail-closed: the generator must emit (1) a DEFERRED identity
/// reset (no eager `rows.removeAll()` before decode), (2) whole-batch rejection
/// on a missing key, (3) whole-batch rejection on an out-of-range state
/// discriminant. These prove the generated cache fails the batch CLOSED rather
/// than emptying the cache / skipping rows / committing a bogus state.
#[test]
fn emits_failclosed_missing_key_and_bad_state_and_deferred_reset() {
    let out = rendered();
    // BLOCKING-2: a row with no key rejects the WHOLE batch (no silent skip).
    assert!(
        out.contains("if row.key == nil {"),
        "missing-key row must reject the whole batch"
    );
    assert!(out.contains("rejecting whole batch"));
    // The old fail-open `guard let key = row.key else { continue }` row-skip must
    // be GONE — no `else { continue }` key-skip anywhere.
    assert!(
        !out.contains("guard let key = row.key else { continue }"),
        "row-skipping on missing key must be removed (fail-closed)"
    );
    // BLOCKING-3 (codex round-2): an out-of-range state discriminant rejects the
    // WHOLE batch — and it MUST do so from the RAW on-wire byte, not the flatc
    // typed accessor `row.state`. The flatc `nmp_refs_RefRow.state` accessor
    // coerces any unknown raw value to `.changed` (`nmp_refs_RefRowState(rawValue:)
    // ?? .changed`), so a `> Cleared` guard against `row.state.rawValue` is DEAD
    // (an on-wire 255 already became 0 before the check). The generator must read
    // the raw discriminant byte directly off the FlatBuffer instead.
    //
    // 1. The dead, fail-OPEN form (guarding the COERCED typed enum) must be GONE.
    assert!(
        !out.contains("if row.state.rawValue > kRefRowStateCleared {"),
        "the fail-open `row.state.rawValue > Cleared` guard (coerced enum) must be removed"
    );
    // 2. A raw-byte reader that bypasses the typed accessor must exist: it walks
    //    the buffer with the public `Table` API and reads `state` as a raw UInt8.
    assert!(
        out.contains("private static func rawRowStateDiscriminants(_ buffer: inout ByteBuffer) -> [UInt8]?"),
        "must emit a raw-state-discriminant reader that bypasses the coercing typed accessor"
    );
    assert!(
        out.contains("let root = Table(bb: buffer, position: rootPosition)"),
        "raw-state reader must build a Table over the verified buffer"
    );
    assert!(
        out.contains("row.readBuffer(of: UInt8.self, at: stateField)"),
        "raw-state reader must read the state field as a RAW UInt8 (no enum coercion)"
    );
    // 3. The whole-batch reject must test the RAW byte against Cleared.
    assert!(
        out.contains("guard let rawStates = Self.rawRowStateDiscriminants(&buffer) else {"),
        "merge must scan the raw discriminants before committing"
    );
    assert!(
        out.contains("if rawState > kRefRowStateCleared {"),
        "unknown RAW state discriminant must reject the whole batch (not coerce to Changed)"
    );
    // 4. A count mismatch between the raw scan and the typed vector also fails closed.
    assert!(
        out.contains("if rawStates.count != batch.rows.count {"),
        "a raw-scan / typed-vector count mismatch must fail the batch closed"
    );
    // BLOCKING-1: identity reset is DEFERRED — there is NO eager full clear at
    // the top of merge; the only `removeAll`/drop happens after a valid decode.
    assert!(
        !out.contains("rows.removeAll()\n            appliedSession = sessionId"),
        "identity reset must be deferred until after a valid baseline decode"
    );
    assert!(
        out.contains("let identityChanged = sessionId != appliedSession || snapshotEpoch != appliedEpoch"),
        "merge must compute identityChanged without clearing the cache"
    );
    // The deferred reset drops other projections only on a successful baseline.
    assert!(out.contains("for k in rows.keys where k != projectionKey { rows.removeValue(forKey: k) }"));
}

#[test]
fn decodes_the_row_delta_batch_payload() {
    let out = rendered();
    assert!(out.contains("batch = try getCheckedRoot(byteBuffer: &buffer, fileId: nmp_refs_RefRowDeltaBatch.id)"));
    assert!(out.contains("for row in batch.rows"));
}

#[test]
fn is_marked_generated() {
    assert!(rendered().contains("THIS FILE IS GENERATED. DO NOT EDIT BY HAND."));
}
