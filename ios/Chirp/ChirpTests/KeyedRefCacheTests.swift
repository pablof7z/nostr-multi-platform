// ADR-0063 Lane A (#1671) — the invariant property tests for the generated
// `KeyedRefCache`, at the generated-cache layer. Mirrors the Rust gate
// `crates/nmp-core/src/refs/tests.rs` and the Kotlin twin
// `android/app/src/test/java/org/nmp/android/KeyedRefCacheTest.kt`.
//
// Each test builds REAL `nmp.refs.RefRowDeltaBatch` FlatBuffers bytes (the same
// wire form the kernel emits) and feeds them to `KeyedRefCache.merge`, so the
// five ADR-0063 invariants are verified against serialized bytes, not literals.

import FlatBuffers
import XCTest

@testable import Chirp

final class KeyedRefCacheTests: XCTestCase {
    private let profile = "refs.profile"
    private let event = "refs.event"

    // MARK: - Batch builder

    private struct Row {
        let key: String
        let rev: UInt64
        let state: nmp_refs_RefRowState
        let payload: [UInt8]
    }

    private func makeBatch(namespace: String, baseline: Bool, rows: [Row]) -> Data {
        var fbb = FlatBufferBuilder()
        var rowOffsets: [Offset] = []
        for row in rows {
            let keyOff = fbb.create(string: row.key)
            let payOff = row.payload.isEmpty ? Offset() : fbb.createVector(row.payload)
            let rowOff = nmp_refs_RefRow.createRefRow(
                &fbb, keyOffset: keyOff, rev: row.rev, state: row.state, payloadVectorOffset: payOff)
            rowOffsets.append(rowOff)
        }
        let rowsVec = fbb.createVector(ofOffsets: rowOffsets)
        let nsOff = fbb.create(string: namespace)
        let batch = nmp_refs_RefRowDeltaBatch.createRefRowDeltaBatch(
            &fbb, namespaceOffset: nsOff, baseline: baseline, rowsVectorOffset: rowsVec)
        nmp_refs_RefRowDeltaBatch.finish(&fbb, end: batch)
        return fbb.data
    }

    private func changed(_ key: String, _ rev: UInt64, _ tag: UInt8) -> Row {
        Row(key: key, rev: rev, state: .changed, payload: [0x01, tag])
    }

    private func cleared(_ key: String, _ rev: UInt64) -> Row {
        Row(key: key, rev: rev, state: .cleared, payload: [])
    }

    // MARK: - Invariant #1: absence is Unchanged, never Cleared

    func testAbsentRowIsRetained() {
        let cache = KeyedRefCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(
                namespace: "profile", baseline: true,
                rows: [changed("alice", 1, 0xAA), changed("bob", 1, 0xBB)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertEqual(cache.count(projectionKey: profile), 2)

        // Only alice changes; bob is ABSENT from the batch and must remain.
        let changed = cache.merge(
            projectionKey: profile,
            payload: makeBatch(
                namespace: "profile", baseline: false, rows: [changed("alice", 2, 0xCC)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertEqual(changed, ["alice"])
        XCTAssertEqual(cache.profile("bob"), Data([0x01, 0xBB]))
        XCTAssertEqual(cache.profile("alice"), Data([0x01, 0xCC]))
    }

    // MARK: - Invariant #2: decode-before-commit keeps prior on malformed

    func testMalformedRowKeepsPriorAndLatchesResync() {
        let cache = KeyedRefCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(
                namespace: "profile", baseline: true,
                rows: [changed("alice", 1, 0xAA), changed("bob", 1, 0xBB)]),
            sessionId: 1, snapshotEpoch: 0)

        // alice's new row is empty (malformed for a Changed row); bob is valid.
        let batch = makeBatch(
            namespace: "profile", baseline: false,
            rows: [
                Row(key: "alice", rev: 2, state: .changed, payload: []),
                changed("bob", 2, 0xEE),
            ])
        let changed = cache.merge(projectionKey: profile, payload: batch, sessionId: 1, snapshotEpoch: 0)
        XCTAssertTrue(cache.needsResync)
        XCTAssertFalse(changed.contains("alice"))
        XCTAssertEqual(cache.profile("alice"), Data([0x01, 0xAA]), "prior row retained")
        XCTAssertEqual(cache.profile("bob"), Data([0x01, 0xEE]), "sibling valid row commits")
    }

    // MARK: - Invariant #3: epoch change → baseline reconstructs full set

    func testEpochBaselineRebuildsFullSet() {
        let cache = KeyedRefCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(
                namespace: "profile", baseline: true,
                rows: [changed("alice", 1, 0xAA), changed("ghost", 1, 0x66)]),
            sessionId: 1, snapshotEpoch: 0)

        // New epoch baseline WITHOUT ghost → cache clears + rebuilds.
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: true, rows: [changed("alice", 2, 0xCC)]),
            sessionId: 1, snapshotEpoch: 1)
        XCTAssertNil(cache.profile("ghost"))
        XCTAssertEqual(cache.profile("alice"), Data([0x01, 0xCC]))
        XCTAssertFalse(cache.needsResync)
    }

    // MARK: - Cleared is explicit

    func testClearedRemovesRow() {
        let cache = KeyedRefCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: true, rows: [changed("alice", 1, 0xAA)]),
            sessionId: 1, snapshotEpoch: 0)
        let changed = cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: false, rows: [cleared("alice", 2)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertEqual(changed, ["alice"])
        XCTAssertNil(cache.profile("alice"))
    }

    // MARK: - Reorder guard

    func testStaleRevIsSkipped() {
        let cache = KeyedRefCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: true, rows: [changed("alice", 5, 0x55)]),
            sessionId: 1, snapshotEpoch: 0)
        let changed = cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: false, rows: [changed("alice", 3, 0x33)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertTrue(changed.isEmpty)
        XCTAssertEqual(cache.profile("alice"), Data([0x01, 0x55]))
    }

    // MARK: - Invariant #4: typed per namespace

    func testNamespaceIsolation() {
        let cache = KeyedRefCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: true, rows: [changed("shared", 1, 0x11)]),
            sessionId: 1, snapshotEpoch: 0)
        cache.merge(
            projectionKey: event,
            payload: makeBatch(namespace: "event", baseline: true, rows: [changed("shared", 1, 0x22)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertEqual(cache.profile("shared"), Data([0x01, 0x11]))
        XCTAssertEqual(cache.event("shared"), Data([0x01, 0x22]))

        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: false, rows: [cleared("shared", 2)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertNil(cache.profile("shared"))
        XCTAssertEqual(cache.event("shared"), Data([0x01, 0x22]), "other namespace untouched")
    }

    // MARK: - Per-row observable: exactly one key notified

    func testRowChangePublisherFiresPerChangedKey() {
        let cache = KeyedRefCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(
                namespace: "profile", baseline: true,
                rows: [changed("alice", 1, 0xAA), changed("bob", 1, 0xBB)]),
            sessionId: 1, snapshotEpoch: 0)

        var observed: [String] = []
        let sub = cache.rowChanged.sink { change in observed.append(change.rowKey) }
        defer { sub.cancel() }

        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: false, rows: [changed("alice", 2, 0xCC)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertEqual(observed, ["alice"], "only the one changed key re-renders")
    }
}
