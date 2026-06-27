// ADR-0063 (#1671) — the invariant property tests for the generated
// `KeyedRefCache`, at the generated-cache layer. Mirrors the Rust gate
// `crates/nmp-core/src/refs/tests.rs` and the Kotlin twin
// `apps/chirp/android/app/src/test/java/org/nmp/android/KeyedRefCacheTest.kt`.
//
// Each test builds REAL `nmp.refs.RefRowDeltaBatch` FlatBuffers bytes (the same
// wire form the kernel emits) and feeds them to `KeyedRefCache.merge`, so the
// five ADR-0063 invariants are verified against serialized bytes, not literals.
//
// Lane A (the five invariants) drives the cache with SYNTHETIC row payloads and
// asserts BYTE storage via the internal `payload(projectionKey:rowKey:)` reader
// — the merge/clear/rebaseline mechanics are payload-agnostic, so the invariants
// must be provable WITHOUT a typed decode. Because Lane C installs a real typed
// decode-before-commit seam, those tests override `rowDecoder` back to the
// permissive Lane-A default so the synthetic bytes commit.
//
// Lane C (#1671 part-(b)) adds `testTypedAccessorDecodesProfileRow` /
// `testTypedAccessorRejectsGarbageRow`, which feed a REAL `KPRF`
// `ProfileSnapshot` row payload (the exact buffer `Kernel::ref_profile_row_payload`
// emits) and assert the TYPED accessor `cache.profile(pubkey) -> ProfileCard?`
// decodes the expected fields — proving the host surface is the typed domain
// type, not Lane A's raw `Data` passthrough.

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

    /// A cache whose decode-before-commit seam is the permissive Lane-A default
    /// (`!payload.isEmpty`), so the SYNTHETIC two-byte row payloads the invariant
    /// tests use commit. Lane C's real typed decoder is exercised separately by
    /// the typed-accessor tests below (with real KPRF buffers).
    private func rawCache() -> KeyedRefCache {
        let cache = KeyedRefCache()
        cache.rowDecoder = { _, payload in !payload.isEmpty }
        return cache
    }

    /// The raw cached row bytes for a `(projectionKey, rowKey)` — the invariant
    /// tests assert byte storage at this layer (payload-agnostic merge mechanics).
    private func rawProfile(_ cache: KeyedRefCache, _ key: String) -> Data? {
        cache.payload(projectionKey: profile, rowKey: key)
    }

    private func rawEvent(_ cache: KeyedRefCache, _ key: String) -> Data? {
        cache.payload(projectionKey: event, rowKey: key)
    }

    // MARK: - Invariant #1: absence is Unchanged, never Cleared

    func testAbsentRowIsRetained() {
        let cache = rawCache()
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
        XCTAssertEqual(rawProfile(cache, "bob"), Data([0x01, 0xBB]))
        XCTAssertEqual(rawProfile(cache, "alice"), Data([0x01, 0xCC]))
    }

    // MARK: - Invariant #2: decode-before-commit keeps prior on malformed

    func testMalformedRowKeepsPriorAndLatchesResync() {
        let cache = rawCache()
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
        XCTAssertEqual(rawProfile(cache, "alice"), Data([0x01, 0xAA]), "prior row retained")
        XCTAssertEqual(rawProfile(cache, "bob"), Data([0x01, 0xEE]), "sibling valid row commits")
    }

    // MARK: - Invariant #3: epoch change → baseline reconstructs full set

    func testEpochBaselineRebuildsFullSet() {
        let cache = rawCache()
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
        XCTAssertNil(rawProfile(cache, "ghost"))
        XCTAssertEqual(rawProfile(cache, "alice"), Data([0x01, 0xCC]))
        XCTAssertFalse(cache.needsResync)
    }

    // MARK: - Cleared is explicit

    func testClearedRemovesRow() {
        let cache = rawCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: true, rows: [changed("alice", 1, 0xAA)]),
            sessionId: 1, snapshotEpoch: 0)
        let changed = cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: false, rows: [cleared("alice", 2)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertEqual(changed, ["alice"])
        XCTAssertNil(rawProfile(cache, "alice"))
    }

    // MARK: - Reorder guard

    func testStaleRevIsSkipped() {
        let cache = rawCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: true, rows: [changed("alice", 5, 0x55)]),
            sessionId: 1, snapshotEpoch: 0)
        let changed = cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: false, rows: [changed("alice", 3, 0x33)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertTrue(changed.isEmpty)
        XCTAssertEqual(rawProfile(cache, "alice"), Data([0x01, 0x55]))
    }

    // MARK: - Invariant #4: typed per namespace

    func testNamespaceIsolation() {
        let cache = rawCache()
        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: true, rows: [changed("shared", 1, 0x11)]),
            sessionId: 1, snapshotEpoch: 0)
        cache.merge(
            projectionKey: event,
            payload: makeBatch(namespace: "event", baseline: true, rows: [changed("shared", 1, 0x22)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertEqual(rawProfile(cache, "shared"), Data([0x01, 0x11]))
        XCTAssertEqual(rawEvent(cache, "shared"), Data([0x01, 0x22]))

        cache.merge(
            projectionKey: profile,
            payload: makeBatch(namespace: "profile", baseline: false, rows: [cleared("shared", 2)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertNil(rawProfile(cache, "shared"))
        XCTAssertEqual(rawEvent(cache, "shared"), Data([0x01, 0x22]), "other namespace untouched")
    }

    // MARK: - Per-row observable: exactly one key notified

    func testRowChangePublisherFiresPerChangedKey() {
        let cache = rawCache()
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

    // MARK: - Lane C (#1671 part-(b)): TYPED per-key accessor

    /// Build a REAL `KPRF` `ProfileSnapshot` row payload — the exact buffer the
    /// kernel's `ref_profile_row_payload` (→ `encode_profile`) emits per row.
    private func makeProfileRowPayload(
        pubkey: String, displayName: String?, pictureUrl: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let pubkeyOff = fbb.create(string: pubkey)
        let dnOff = displayName.map { fbb.create(string: $0) } ?? Offset()
        let purlOff = pictureUrl.map { fbb.create(string: $0) } ?? Offset()
        // nip05 / about are non-optional strings on the wire (empty when absent).
        let nip05Off = fbb.create(string: "")
        let aboutOff = fbb.create(string: "")
        let cardStart = nmp_kernel_ProfileCard.startProfileCard(&fbb)
        nmp_kernel_ProfileCard.add(pubkey: pubkeyOff, &fbb)
        if displayName != nil {
            nmp_kernel_ProfileCard.add(hasDisplayName: true, &fbb)
            nmp_kernel_ProfileCard.add(displayName: dnOff, &fbb)
        }
        if pictureUrl != nil {
            nmp_kernel_ProfileCard.add(hasPictureUrl: true, &fbb)
            nmp_kernel_ProfileCard.add(pictureUrl: purlOff, &fbb)
        }
        nmp_kernel_ProfileCard.add(nip05: nip05Off, &fbb)
        nmp_kernel_ProfileCard.add(about: aboutOff, &fbb)
        let cardOff = nmp_kernel_ProfileCard.endProfileCard(&fbb, start: cardStart)
        let snapOff = nmp_kernel_ProfileSnapshot.createProfileSnapshot(&fbb, cardOffset: cardOff)
        nmp_kernel_ProfileSnapshot.finish(&fbb, end: snapOff)
        return Array(fbb.data)
    }

    /// The typed accessor `cache.profile(pubkey) -> ProfileCard?` decodes the
    /// cached KPRF row payload into the concrete domain type (NOT raw `Data`).
    func testTypedAccessorDecodesProfileRow() {
        let cache = KeyedRefCache()  // real typed decode-before-commit seam
        let payload = makeProfileRowPayload(
            pubkey: "abc123", displayName: "Alice", pictureUrl: "https://example/a.png")
        let changed = cache.merge(
            projectionKey: profile,
            payload: makeBatch(
                namespace: "profile", baseline: true,
                rows: [Row(key: "abc123", rev: 1, state: .changed, payload: payload)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertEqual(changed, ["abc123"])
        XCTAssertFalse(cache.needsResync, "a valid KPRF row commits without latching resync")

        let card: ProfileCard? = cache.profile("abc123")
        XCTAssertEqual(card?.pubkey, "abc123")
        XCTAssertEqual(card?.displayName, "Alice")
        XCTAssertEqual(card?.pictureUrl, "https://example/a.png")
        XCTAssertNil(cache.profile("missing"), "an absent key decodes to nil")
    }

    /// A garbage (non-KPRF) `Changed` row fails the real typed decode-before-
    /// commit seam: the row is NOT committed and `needsResync` latches.
    func testTypedAccessorRejectsGarbageRow() {
        let cache = KeyedRefCache()  // real typed decode-before-commit seam
        let changed = cache.merge(
            projectionKey: profile,
            payload: makeBatch(
                namespace: "profile", baseline: true,
                rows: [Row(key: "abc123", rev: 1, state: .changed, payload: [0xDE, 0xAD, 0xBE, 0xEF])]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertTrue(changed.isEmpty, "garbage row is not committed")
        XCTAssertTrue(cache.needsResync, "typed decode failure latches resync")
        XCTAssertNil(cache.profile("abc123"))
    }

    // MARK: - Lane C (#1671) BLOCKING: refs.event single-entry fail-closed

    /// Build a `KCEV` `ClaimedEventsSnapshot` row payload carrying `count` entries.
    /// The kernel's `ref_event_row_payload` ALWAYS encodes EXACTLY ONE entry per
    /// `refs.event` row; this helper can forge 0, 1, or 2+ entries to prove the
    /// glue's single-entry contract.
    private func makeEventRowPayload(entryKeys: [String], signedEventJson: String? = nil) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        var entryOffsets: [Offset] = []
        for key in entryKeys {
            let idOff = fbb.create(string: key)
            let authorOff = fbb.create(string: "author-\(key)")
            let contentOff = fbb.create(string: "content-\(key)")
            let primaryOff = fbb.create(string: key)
            let signedJsonOff = signedEventJson.map { fbb.create(string: $0) } ?? Offset()
            let eventOff = nmp_kernel_ClaimedEvent.createClaimedEvent(
                &fbb,
                primaryIdOffset: primaryOff,
                idOffset: idOff,
                authorPubkeyOffset: authorOff,
                kind: 1,
                createdAt: 100,
                contentOffset: contentOff,
                hasSignedEventJson: signedEventJson != nil,
                signedEventJsonOffset: signedJsonOff)
            let keyOff = fbb.create(string: key)
            entryOffsets.append(
                nmp_kernel_ClaimedEventEntry.createClaimedEventEntry(
                    &fbb, keyOffset: keyOff, valueOffset: eventOff))
        }
        let entriesVec = fbb.createVector(ofOffsets: entryOffsets)
        let snapOff = nmp_kernel_ClaimedEventsSnapshot.createClaimedEventsSnapshot(
            &fbb, entriesVectorOffset: entriesVec)
        nmp_kernel_ClaimedEventsSnapshot.finish(&fbb, end: snapOff)
        return Array(fbb.data)
    }

    /// SANITY: a well-formed single-entry KCEV row decodes through the typed
    /// `event(primaryId) -> ClaimedEventDto?` accessor.
    func testTypedEventAccessorDecodesSingleEntryRow() {
        let cache = KeyedRefCache()  // real typed decode-before-commit seam
        let changed = cache.merge(
            projectionKey: event,
            payload: makeBatch(
                namespace: "event", baseline: true,
                rows: [Row(key: "evt1", rev: 1, state: .changed, payload: makeEventRowPayload(
                    entryKeys: ["evt1"],
                    signedEventJson: #"{"id":"evt1","sig":"signed"}"#))]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertEqual(changed, ["evt1"])
        XCTAssertFalse(cache.needsResync, "a valid single-entry KCEV row commits cleanly")
        let dto: ClaimedEventDto? = cache.event("evt1")
        XCTAssertEqual(dto?.id, "evt1")
        XCTAssertEqual(dto?.authorPubkey, "author-evt1")
        XCTAssertEqual(dto?.content, "content-evt1")
        XCTAssertEqual(dto?.signedEventJson, #"{"id":"evt1","sig":"signed"}"#)
    }

    /// BLOCKING (codex): a MULTI-entry KCEV row violates the kernel's exactly-one
    /// `refs.event` row contract (`ref_event_row_payload` always emits one entry).
    /// `refRowEvent` must reject it — `reader.entries.count == 1` fails, so the
    /// typed decode-before-commit seam treats the row as malformed: it is NOT
    /// committed, `needsResync` latches, and the accessor returns nil. This proves
    /// a forged 2-entry buffer can never silently commit its first entry.
    func testTypedEventAccessorRejectsMultiEntryRow() {
        let cache = KeyedRefCache()  // real typed decode-before-commit seam
        let twoEntry = makeEventRowPayload(entryKeys: ["evtA", "evtB"])
        let changed = cache.merge(
            projectionKey: event,
            payload: makeBatch(
                namespace: "event", baseline: true,
                rows: [Row(key: "evtA", rev: 1, state: .changed, payload: twoEntry)]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertTrue(changed.isEmpty, "a multi-entry KCEV row must NOT commit")
        XCTAssertTrue(cache.needsResync, "the single-entry-contract violation latches resync")
        XCTAssertNil(cache.event("evtA"), "no entry from a malformed multi-entry row is committed")
    }

    /// A ZERO-entry KCEV row is likewise malformed (no event to bind): rejected,
    /// not committed.
    func testTypedEventAccessorRejectsEmptyRow() {
        let cache = KeyedRefCache()  // real typed decode-before-commit seam
        let changed = cache.merge(
            projectionKey: event,
            payload: makeBatch(
                namespace: "event", baseline: true,
                rows: [Row(key: "evtA", rev: 1, state: .changed, payload: makeEventRowPayload(entryKeys: []))]),
            sessionId: 1, snapshotEpoch: 0)
        XCTAssertTrue(changed.isEmpty, "a zero-entry KCEV row must NOT commit")
        XCTAssertTrue(cache.needsResync, "the single-entry-contract violation latches resync")
        XCTAssertNil(cache.event("evtA"))
    }
}
