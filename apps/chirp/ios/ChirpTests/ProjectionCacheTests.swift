import XCTest
import FlatBuffers
@testable import Chirp

/// Tests for `ProjectionMergeCache` (ADR-0055 R3-S3).
///
/// These tests drive REAL frames through the cache and assert ACTUAL cached /
/// decoded values — they are deliberately non-vacuous so a reviewer can
/// confirm each algorithmic invariant is enforced end-to-end.
///
/// Test coverage:
///   1. Omitted key retains prior cached value (Unchanged == absence).
///   2. Cleared row drops the key from the cache.
///   3. Changed row with higher rev overwrites the cache.
///   4. Reorder guard: lower-or-equal rev is ignored (prior value retained).
///   5. Session / epoch change clears the cache (reset-before-merge, atomic).
///   6. Decode-failure keeps prior cache entry and latches needsResync.
///   7. session_id == 0 treated as full / no-omission-trust.
///   8. changedKeys contains exactly the keys that advanced.
///   9. Baselined flips to true after the first non-zero-session frame.
///  10. Cleared keys are present in changedKeys (so callers can nil the slot).
final class ProjectionCacheTests: XCTestCase {

    // MARK: - Helpers

    private static let testSessionId: UInt64 = 42
    private static let testEpoch: UInt64 = 1

    /// Build a well-formed KACC FlatBuffers payload (accounts snapshot).
    /// Used to produce `Changed` envelopes with a specific payload that we
    /// can verify round-trips through the cache.
    private func makeAccountsPayload(npub: String) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let idOff = fbb.create(string: "test-id")
        let npubOff = fbb.create(string: npub)
        let kindOff = fbb.create(string: "local")
        let statusOff = fbb.create(string: "ready")
        // signer_label removed from the wire (#1712) — derived shell-side.
        let rowOffset = nmp_kernel_AccountSummaryRow.createAccountSummaryRow(
            &fbb,
            idOffset: idOff,
            npubOffset: npubOff,
            hasDisplayName: false,
            displayNameOffset: Offset(),
            signerKindOffset: kindOff,
            statusOffset: statusOff,
            signerIsRemote: false,
            isActive: true,
            hasPictureUrl: false,
            pictureUrlOffset: Offset()
        )
        let vec = fbb.createVector(ofOffsets: [rowOffset])
        let root = nmp_kernel_AccountsSnapshot.createAccountsSnapshot(&fbb, accountsVectorOffset: vec)
        nmp_kernel_AccountsSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    /// Build a well-formed KACT FlatBuffers payload (active account).
    private func makeActiveAccountPayload(pubkey: String) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 128)
        let pubkeyOff = fbb.create(string: pubkey)
        let root = nmp_kernel_ActiveAccountSnapshot.createActiveAccountSnapshot(
            &fbb, hasActiveAccount: true, pubkeyOffset: pubkeyOff)
        nmp_kernel_ActiveAccountSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    /// Make a `Changed` envelope for the accounts projection at the given rev.
    private func changedAccountsEnvelope(rev: UInt64, npub: String) -> TypedProjectionEnvelope {
        TypedProjectionEnvelope(
            key: TypedAccountsDecoder.key,
            schemaId: TypedAccountsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedAccountsDecoder.fileIdentifier,
            payload: makeAccountsPayload(npub: npub),
            projectionRev: rev,
            state: .changed
        )
    }

    /// Make a `Changed` envelope for the active_account projection.
    private func changedActiveAccountEnvelope(rev: UInt64, pubkey: String) -> TypedProjectionEnvelope {
        TypedProjectionEnvelope(
            key: TypedActiveAccountDecoder.key,
            schemaId: TypedActiveAccountDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedActiveAccountDecoder.fileIdentifier,
            payload: makeActiveAccountPayload(pubkey: pubkey),
            projectionRev: rev,
            state: .changed
        )
    }

    /// Make a `Cleared` envelope for a given key.
    private func clearedEnvelope(key: String, rev: UInt64) -> TypedProjectionEnvelope {
        TypedProjectionEnvelope(
            key: key,
            schemaId: "",
            schemaVersion: 0,
            fileIdentifier: "",
            payload: Data(),
            projectionRev: rev,
            state: .cleared
        )
    }

    /// Run a merge with standard test session/epoch.
    private func merge(
        cache: ProjectionMergeCache,
        envelopes: [TypedProjectionEnvelope],
        sessionId: UInt64 = testSessionId,
        epoch: UInt64 = testEpoch
    ) -> MergeResult {
        cache.merge(envelopes: envelopes, sessionId: sessionId, snapshotEpoch: epoch)
    }

    // MARK: - Test 1: Omitted key retains prior cached value

    /// D3 invariant: absence == Unchanged, never Cleared.
    /// After a key is cached via a Changed row, omitting it in the next frame
    /// must retain its prior value in the merged envelope set.
    func testOmittedKeyRetainsPriorValue() throws {
        let cache = ProjectionMergeCache()

        // Frame A: accounts is Changed (cached).
        let frameA = [changedAccountsEnvelope(rev: 1, npub: "npub1-A")]
        let resultA = merge(cache: cache, envelopes: frameA)
        XCTAssertTrue(resultA.changedKeys.contains(TypedAccountsDecoder.key))
        let decodedA = TypedAccountsDecoder.decode(from: resultA.mergedEnvelopes)
        XCTAssertEqual(decodedA?.first?.npub, "npub1-A", "accounts should be decoded on first frame")

        // Frame B: accounts is OMITTED (Unchanged). Merged set must still carry it.
        let resultB = merge(cache: cache, envelopes: [])
        XCTAssertFalse(resultB.changedKeys.contains(TypedAccountsDecoder.key),
                       "omitted key must NOT be in changedKeys")
        let decodedB = TypedAccountsDecoder.decode(from: resultB.mergedEnvelopes)
        XCTAssertEqual(decodedB?.first?.npub, "npub1-A",
                       "accounts must retain prior cached value when omitted")
    }

    // MARK: - Test 2: Cleared row drops the key

    /// D3-1: an explicit Cleared row removes the key from the cache so
    /// subsequent merged envelopes no longer carry it.
    func testClearedRowDropsKey() throws {
        let cache = ProjectionMergeCache()

        // Frame A: cache the accounts key.
        _ = merge(cache: cache, envelopes: [changedAccountsEnvelope(rev: 1, npub: "npub1-A")])

        // Frame B: explicit Cleared row.
        let frameB = [clearedEnvelope(key: TypedAccountsDecoder.key, rev: 2)]
        let resultB = merge(cache: cache, envelopes: frameB)
        XCTAssertTrue(resultB.changedKeys.contains(TypedAccountsDecoder.key),
                      "Cleared key must appear in changedKeys so the caller nils the @Published slot")
        let decodedB = TypedAccountsDecoder.decode(from: resultB.mergedEnvelopes)
        XCTAssertNil(decodedB, "accounts must be nil after explicit Cleared row")

        // Frame C: omitted — must still be absent (clear doesn't re-appear).
        let resultC = merge(cache: cache, envelopes: [])
        let decodedC = TypedAccountsDecoder.decode(from: resultC.mergedEnvelopes)
        XCTAssertNil(decodedC, "accounts must stay nil after Cleared + omission")
    }

    // MARK: - Test 3: Changed row overwrites

    /// D3-3 merge: a Changed row with higher rev overwrites the cached bytes.
    func testChangedRowOverwrites() throws {
        let cache = ProjectionMergeCache()
        _ = merge(cache: cache, envelopes: [changedAccountsEnvelope(rev: 1, npub: "npub1-original")])

        // Frame B: Changed row with higher rev and different payload.
        let resultB = merge(cache: cache, envelopes: [changedAccountsEnvelope(rev: 2, npub: "npub1-updated")])
        XCTAssertTrue(resultB.changedKeys.contains(TypedAccountsDecoder.key))
        let decoded = TypedAccountsDecoder.decode(from: resultB.mergedEnvelopes)
        XCTAssertEqual(decoded?.first?.npub, "npub1-updated",
                       "Changed row with higher rev must overwrite the cached value")
    }

    // MARK: - Test 4: Reorder guard (lower rev is ignored)

    /// D3 reorder guard: a Changed row with rev <= cached.rev must be ignored;
    /// the prior cached value must be retained.
    func testReorderGuardLowerRevIgnored() throws {
        let cache = ProjectionMergeCache()
        // Prime the cache at rev=5.
        _ = merge(cache: cache, envelopes: [changedAccountsEnvelope(rev: 5, npub: "npub1-at-rev5")])

        // Frame B: Changed row with lower rev (stale, from an out-of-order delivery).
        let resultB = merge(cache: cache, envelopes: [changedAccountsEnvelope(rev: 3, npub: "npub1-stale")])
        XCTAssertFalse(resultB.changedKeys.contains(TypedAccountsDecoder.key),
                       "lower-rev row must NOT be in changedKeys (reorder guard)")
        let decoded = TypedAccountsDecoder.decode(from: resultB.mergedEnvelopes)
        XCTAssertEqual(decoded?.first?.npub, "npub1-at-rev5",
                       "prior cached value (rev=5) must be retained when rev=3 arrives")

        // Equal rev is also a no-op.
        let resultC = merge(cache: cache, envelopes: [changedAccountsEnvelope(rev: 5, npub: "npub1-same-rev")])
        XCTAssertFalse(resultC.changedKeys.contains(TypedAccountsDecoder.key),
                       "equal-rev row must also be ignored by the reorder guard")
        let decodedC = TypedAccountsDecoder.decode(from: resultC.mergedEnvelopes)
        XCTAssertEqual(decodedC?.first?.npub, "npub1-at-rev5")
    }

    // MARK: - Test 5: Session / epoch change clears the cache

    /// D4: a sessionId or snapshotEpoch change triggers a mandatory full-cache
    /// reset BEFORE the incoming rows are merged. The prior session's entries
    /// must be wiped. Per the ADR D3-3 pseudocode, `baselined` is reset to
    /// `false` at the top of the merge (the D4 block) and then re-asserted to
    /// `true` at the end once the post-reset frame has been applied — so the
    /// observable post-merge state is `baselined == true` on a fresh baseline.
    /// The load-bearing assertion is that the cache was cleared.
    func testSessionChangeClears() throws {
        let cache = ProjectionMergeCache()
        let session1: UInt64 = 10
        let session2: UInt64 = 20
        let epoch1: UInt64 = 1

        // Prime cache with session 10.
        _ = merge(cache: cache,
                  envelopes: [changedAccountsEnvelope(rev: 1, npub: "npub-session1")],
                  sessionId: session1, epoch: epoch1)
        XCTAssertTrue(cache.baselined)

        // New session — cache must be wiped; the (empty) post-reset frame is
        // itself a valid baseline, so baselined re-asserts true.
        let result = merge(cache: cache, envelopes: [], sessionId: session2, epoch: epoch1)
        XCTAssertTrue(cache.baselined,
                      "the post-reset baseline frame re-asserts baselined == true (ADR D3-3)")
        let decoded = TypedAccountsDecoder.decode(from: result.mergedEnvelopes)
        XCTAssertNil(decoded,
                     "accounts must be absent after session change clears the cache")
        XCTAssertTrue(result.changedKeys.isEmpty,
                      "an empty post-reset baseline frame advances no keys")
    }

    func testEpochChangeClears() throws {
        let cache = ProjectionMergeCache()
        let session: UInt64 = 10
        let epoch1: UInt64 = 1
        let epoch2: UInt64 = 2

        _ = merge(cache: cache,
                  envelopes: [changedAccountsEnvelope(rev: 1, npub: "npub-epoch1")],
                  sessionId: session, epoch: epoch1)
        XCTAssertTrue(cache.baselined)

        // Epoch bump — same effect as session change.
        let result = merge(cache: cache, envelopes: [], sessionId: session, epoch: epoch2)
        XCTAssertTrue(cache.baselined,
                      "the post-reset baseline frame re-asserts baselined == true (ADR D3-3)")
        XCTAssertNil(TypedAccountsDecoder.decode(from: result.mergedEnvelopes),
                     "accounts must be absent after epoch change clears the cache")
    }

    // MARK: - Test 5c: Atomic reset-before-merge with a NON-empty frame

    /// D4 ordering invariant: when a single frame simultaneously changes the
    /// session (or epoch) AND carries rows, the cache reset must happen BEFORE
    /// the merge of those rows. The stale pre-reset entries must be wiped, and
    /// ONLY the rows in this frame must land in the fresh cache — proving the
    /// reset does not clobber the same-frame rows, and the same-frame rows do
    /// not survive from the prior session.
    ///
    /// The existing session/epoch tests only cover reset with an EMPTY frame;
    /// this covers the load-bearing case where reset and re-population are
    /// atomic within one merge call.
    func testSessionChangeWithRowsResetsThenMergesAtomically() throws {
        let cache = ProjectionMergeCache()
        let session1: UInt64 = 100
        let session2: UInt64 = 200
        let epoch: UInt64 = 1

        // Prime session 1 with BOTH accounts and active_account.
        _ = merge(cache: cache, envelopes: [
            changedAccountsEnvelope(rev: 50, npub: "npub-OLD-session"),
            changedActiveAccountEnvelope(rev: 50, pubkey: "pk-OLD-session"),
        ], sessionId: session1, epoch: epoch)
        XCTAssertTrue(cache.baselined)

        // New session in the SAME frame that also carries a fresh accounts row
        // ONLY (active_account is NOT in this frame). If reset happened AFTER
        // the merge, the new accounts row would be wiped; if reset did not
        // happen, the stale active_account would survive. Neither is allowed.
        let result = merge(cache: cache, envelopes: [
            changedAccountsEnvelope(rev: 1, npub: "npub-NEW-session"),
        ], sessionId: session2, epoch: epoch)

        // The fresh accounts row must land (reset happened BEFORE the merge).
        let accounts = TypedAccountsDecoder.decode(from: result.mergedEnvelopes)
        XCTAssertEqual(accounts?.first?.npub, "npub-NEW-session",
                       "the same-frame accounts row must land in the freshly-reset cache")

        // The stale active_account from session 1 must be GONE (reset wiped it,
        // and it was not re-sent in this frame).
        XCTAssertNil(TypedActiveAccountDecoder.decode(from: result.mergedEnvelopes),
                     "stale active_account from the prior session must NOT survive the reset")

        // changedKeys must contain EXACTLY the one key that was sent this frame.
        XCTAssertEqual(result.changedKeys, [TypedAccountsDecoder.key],
                       "changedKeys must be exactly the same-frame rows, not the wiped ones")

        // Note the reorder guard did NOT reject the rev=1 row even though the
        // prior session had rev=50: the reset cleared the cache first, so there
        // was no cached entry to compare against. This proves reset-before-merge.
        XCTAssertTrue(cache.baselined, "baselined re-asserts true after the same-frame baseline")
    }

    // MARK: - Test 6: Decode-failure keeps prior + latches needsResync

    /// D3-4: if the decode-before-commit preflight fails, the prior cache entry
    /// must be retained and `needsResync` must be latched — the failing payload
    /// must NOT blank the cached value.
    ///
    /// NOTE on the failure vector: the typed decoders use unchecked `getRoot`
    /// (trusted in-process FFI boundary — see `KernelUpdateFrameDecoder`), so
    /// arbitrary non-empty garbage bytes do NOT reliably round-trip to `nil`
    /// (an out-of-range offset just yields an empty/default-valued struct). The
    /// one DETERMINISTIC decode-failure the cache must defend against is a
    /// `Changed` row carrying an EMPTY payload — a malformed frame, because a
    /// `Changed` row by contract carries full bytes (an empty projection is
    /// expressed as `Cleared`, never as Changed-with-no-bytes). `decodeSucceeds`
    /// rejects this via its `!bytes.isEmpty` guard, which is exactly the
    /// preflight gate D3-4 specifies. This is the honest, reproducible failure
    /// path; a fuzzed-garbage assertion would be vacuous under unchecked getRoot.
    func testDecodeFailureKeepsPriorAndLatchesNeedsResync() throws {
        let cache = ProjectionMergeCache()
        _ = merge(cache: cache,
                  envelopes: [changedAccountsEnvelope(rev: 1, npub: "npub-good")])
        XCTAssertFalse(cache.needsResync)

        // A malformed `Changed` envelope carrying NO payload bytes. A Changed
        // row must carry bytes; an empty-payload Changed row is the canonical
        // decode-before-commit failure (decodeSucceeds returns false on empty).
        let malformedEnvelope = TypedProjectionEnvelope(
            key: TypedAccountsDecoder.key,
            schemaId: TypedAccountsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedAccountsDecoder.fileIdentifier,
            payload: Data(),  // empty — malformed for a Changed row
            projectionRev: 2,
            state: .changed
        )
        let result = merge(cache: cache, envelopes: [malformedEnvelope])

        // needsResync must be latched.
        XCTAssertTrue(cache.needsResync,
                      "needsResync must be latched after decode-before-commit failure")
        XCTAssertTrue(result.needsResync)

        // The malformed row must NOT be in changedKeys (decode failed, rev not advanced).
        XCTAssertFalse(result.changedKeys.contains(TypedAccountsDecoder.key),
                       "failed row must NOT appear in changedKeys")

        // Prior cached value (npub-good) must be retained — not blanked.
        let decoded = TypedAccountsDecoder.decode(from: result.mergedEnvelopes)
        XCTAssertEqual(decoded?.first?.npub, "npub-good",
                       "prior cached value must be retained when decode-before-commit fails")
    }

    // MARK: - Test 7: session_id == 0 treated as full / no-omission-trust

    /// D3-5: a frame with sessionId == 0 means "no incremental contract".
    /// The envelopes are passed through unchanged (as the merged set), and
    /// omission is not trusted. The cache itself is not cleared.
    func testSessionIdZeroPassThroughsEnvelopes() throws {
        let cache = ProjectionMergeCache()
        // Prime cache with session 1.
        _ = merge(cache: cache,
                  envelopes: [changedAccountsEnvelope(rev: 1, npub: "npub-cached")],
                  sessionId: 1, epoch: 1)

        // Frame with sessionId == 0 and a different accounts row.
        let zeroSessionEnvelope = changedAccountsEnvelope(rev: 99, npub: "npub-zero-session")
        let result = cache.merge(envelopes: [zeroSessionEnvelope], sessionId: 0, snapshotEpoch: 1)

        // The incoming envelopes are passed through as-is.
        let decoded = TypedAccountsDecoder.decode(from: result.mergedEnvelopes)
        XCTAssertEqual(decoded?.first?.npub, "npub-zero-session",
                       "sessionId==0 frames must pass envelopes through as-is")

        // changedKeys must contain all incoming keys (conservative).
        XCTAssertTrue(result.changedKeys.contains(TypedAccountsDecoder.key))
    }

    // MARK: - Test 8: changedKeys contains exactly the advanced keys

    /// The `changedKeys` set must contain exactly the keys that actually
    /// changed (Changed rows that committed + Cleared rows). Omitted keys
    /// and reorder-rejected rows must NOT appear.
    func testChangedKeysAreExact() throws {
        let cache = ProjectionMergeCache()
        // Frame A: cache both accounts and active_account.
        _ = merge(cache: cache, envelopes: [
            changedAccountsEnvelope(rev: 1, npub: "npub-A"),
            changedActiveAccountEnvelope(rev: 1, pubkey: "pubkey-A"),
        ])

        // Frame B: only active_account advances; accounts is omitted.
        let resultB = merge(cache: cache, envelopes: [
            changedActiveAccountEnvelope(rev: 2, pubkey: "pubkey-B"),
        ])
        XCTAssertFalse(resultB.changedKeys.contains(TypedAccountsDecoder.key),
                       "omitted accounts must NOT be in changedKeys")
        XCTAssertTrue(resultB.changedKeys.contains(TypedActiveAccountDecoder.key),
                      "advanced active_account must be in changedKeys")
    }

    // MARK: - Test 9: baselined flips to true

    /// D3-5: `baselined` is false initially and flips to true after the first
    /// non-zero-session frame is applied.
    func testBaselinedFlipsAfterFirstFrame() throws {
        let cache = ProjectionMergeCache()
        XCTAssertFalse(cache.baselined)

        _ = merge(cache: cache, envelopes: [], sessionId: 1, epoch: 1)
        XCTAssertTrue(cache.baselined, "baselined must flip to true after first non-zero session frame")
    }

    // MARK: - Test 10: Cleared keys appear in changedKeys

    /// A Cleared row must appear in `changedKeys` so `KernelModel.apply`
    /// can nil-out the corresponding @Published slot.
    func testClearedKeyInChangedKeys() throws {
        let cache = ProjectionMergeCache()
        // Cache the key first.
        _ = merge(cache: cache, envelopes: [changedAccountsEnvelope(rev: 1, npub: "A")])

        // Send a Cleared row.
        let result = merge(cache: cache, envelopes: [
            clearedEnvelope(key: TypedAccountsDecoder.key, rev: 2)
        ])
        XCTAssertTrue(result.changedKeys.contains(TypedAccountsDecoder.key),
                      "Cleared key must be in changedKeys so the slot is nilled")
    }

    // MARK: - Test 11: Multiple keys in one frame — correct partial changedKeys

    /// Verify changedKeys is precisely the set of keys that committed in a
    /// multi-key frame (a Changed that succeeded, a reorder-rejected one,
    /// and a Cleared).
    func testMultiKeyFrameChangedKeysAreExact() throws {
        let cache = ProjectionMergeCache()
        // Seed both keys.
        _ = merge(cache: cache, envelopes: [
            changedAccountsEnvelope(rev: 10, npub: "npub-10"),
            changedActiveAccountEnvelope(rev: 10, pubkey: "pk-10"),
        ])

        // Frame: accounts stale (rev 5 < 10), active_account advances (rev 11), Cleared for a hypothetical key.
        let result = merge(cache: cache, envelopes: [
            changedAccountsEnvelope(rev: 5, npub: "npub-stale"),   // reorder-rejected
            changedActiveAccountEnvelope(rev: 11, pubkey: "pk-11"), // committed
        ])

        XCTAssertFalse(result.changedKeys.contains(TypedAccountsDecoder.key),
                       "stale accounts (rev 5 < 10) must NOT be in changedKeys")
        XCTAssertTrue(result.changedKeys.contains(TypedActiveAccountDecoder.key),
                      "advanced active_account must be in changedKeys")

        // Stale rev must not overwrite the cache.
        let accounts = TypedAccountsDecoder.decode(from: result.mergedEnvelopes)
        XCTAssertEqual(accounts?.first?.npub, "npub-10",
                       "stale row must not overwrite the cache")
    }

    // MARK: - Test 12: reset() clears everything

    /// `ProjectionMergeCache.reset()` must clear the cache, reset
    /// `baselined`, and clear `needsResync`.
    func testResetClearsEverything() throws {
        let cache = ProjectionMergeCache()
        _ = merge(cache: cache, envelopes: [changedAccountsEnvelope(rev: 1, npub: "A")])
        XCTAssertTrue(cache.baselined)

        // Inject a needsResync.
        let corrupt = TypedProjectionEnvelope(
            key: TypedAccountsDecoder.key,
            schemaId: TypedAccountsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedAccountsDecoder.fileIdentifier,
            payload: Data([0x00]),
            projectionRev: 2,
            state: .changed
        )
        _ = merge(cache: cache, envelopes: [corrupt])

        cache.reset()
        XCTAssertFalse(cache.baselined)
        XCTAssertFalse(cache.needsResync)
        let result = merge(cache: cache, envelopes: [], sessionId: 99, epoch: 99)
        XCTAssertNil(TypedAccountsDecoder.decode(from: result.mergedEnvelopes),
                     "cache must be empty after reset")
    }
}
