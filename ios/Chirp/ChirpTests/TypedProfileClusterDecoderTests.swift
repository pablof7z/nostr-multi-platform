import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the profile-cluster sidecars: `profile` (`KPRF`),
/// `claimed_profiles` (`KCPR`), and `resolved_profiles` (`KRPR`). All three
/// share ONE `nmp_kernel_ProfileCard` reader (defined once in
/// `ProfileCard.generated.swift`, `include`d by each per-key schema) — the
/// shared-binding refactor that this batch landed first.
///
/// These mirror `TypedAppProjectionsDecoderTests`: build the typed FlatBuffers
/// buffer directly via the generated builders, wrap it in a
/// `TypedProjectionEnvelope` carrying the producer's `(key, schemaId)`, and
/// assert the generated `Typed<Key>Decoder` produces the Chirp domain value.
///
/// PRECEDENCE CONTRACT: the typed value must be USED, not merely decodable. Each
/// "typed present" case uses values that DIFFER from any plausible JSON value,
/// so a passing assertion proves the typed path won rather than coincided. The
/// "absent / wrong-schema / garbled" cases assert `nil`, the signal the read
/// site interprets as "fall back to the generic JSON `projections.<field>` path"
/// (ADR-0037 Commitment 4).
///
/// The `has_*` companion-bool semantics are pinned in
/// `testProfileCardHasFlagsMapToNilOptionals`: when `has_x == false` the
/// corresponding Chirp optional is `nil`, reproducing the JSON
/// `null`-when-`None` shape regardless of the (empty) string slot.
final class TypedProfileClusterDecoderTests: XCTestCase {

    // A fully-populated card whose values are distinct enough to prove the
    // typed path is what produced them.
    private static let fullCard = CardFields(
        pubkey: "typedpubkey00",
        displayName: "Typed Display",
        pictureUrl: "https://typed.example/pic.png",
        nip05: "typed@example.com",
        about: "typed about text",
        lnurl: "typed@walletofsatoshi.com")

    // MARK: - profile (KPRF)

    func testProfileSidecarIdentityIsExact() {
        XCTAssertEqual(TypedProfileDecoder.key, "profile")
        XCTAssertEqual(TypedProfileDecoder.schemaId, "profile")
        XCTAssertEqual(TypedProfileDecoder.fileIdentifier, "KPRF")
    }

    func testTypedProfileSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedProfileDecoder.key,
            schemaId: TypedProfileDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedProfileDecoder.fileIdentifier,
            payload: buildProfile(Self.fullCard))

        let card = try XCTUnwrap(
            TypedProfileDecoder.decode(from: [envelope]),
            "well-formed KPRF sidecar must decode")

        assertEqual(card, Self.fullCard)
    }

    func testAbsentProfileSidecarFallsBack() {
        XCTAssertNil(TypedProfileDecoder.decode(from: []))
    }

    func testWrongSchemaProfileFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedProfileDecoder.key,
            schemaId: "not.profile",
            schemaVersion: 1,
            fileIdentifier: TypedProfileDecoder.fileIdentifier,
            payload: buildProfile(Self.fullCard))
        XCTAssertNil(TypedProfileDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    /// `has_display_name` / `has_picture_url` / `has_lnurl` == false must map to
    /// `nil` optionals (JSON `null`-when-`None` parity), regardless of the empty
    /// string slot, while the always-present scalar strings stay non-nil.
    func testProfileCardHasFlagsMapToNilOptionals() throws {
        let placeholder = CardFields(
            pubkey: "barepk",
            displayName: nil,
            pictureUrl: nil,
            nip05: "",
            about: "",
            lnurl: nil)
        let card = try XCTUnwrap(TypedProfileDecoder.decode(bytes: buildProfile(placeholder)))

        XCTAssertEqual(card.pubkey, "barepk")
        XCTAssertNil(card.displayName)
        XCTAssertNil(card.pictureUrl)
        XCTAssertNil(card.lnurl)
        XCTAssertEqual(card.nip05, "")
        XCTAssertEqual(card.about, "")
    }

    // MARK: - claimed_profiles (KCPR)

    func testClaimedProfilesSidecarIdentityIsExact() {
        XCTAssertEqual(TypedClaimedProfilesDecoder.key, "claimed_profiles")
        XCTAssertEqual(TypedClaimedProfilesDecoder.schemaId, "claimed_profiles")
        XCTAssertEqual(TypedClaimedProfilesDecoder.fileIdentifier, "KCPR")
    }

    func testTypedClaimedProfilesSidecarDecodes() throws {
        let a = CardFields.simple(pubkey: "pkA", display: "Alice")
        let b = CardFields.simple(pubkey: "pkB", display: "Bob")
        let envelope = TypedProjectionEnvelope(
            key: TypedClaimedProfilesDecoder.key,
            schemaId: TypedClaimedProfilesDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedClaimedProfilesDecoder.fileIdentifier,
            payload: buildProfileMap(buildClaimedProfiles, [("pkA", a), ("pkB", b)]))

        let map = try XCTUnwrap(TypedClaimedProfilesDecoder.decode(from: [envelope]))

        XCTAssertEqual(Set(map.keys), ["pkA", "pkB"])
        XCTAssertEqual(map["pkA"]?.displayName, "Alice")
        XCTAssertEqual(map["pkB"]?.displayName, "Bob")
    }

    func testAbsentClaimedProfilesSidecarFallsBack() {
        XCTAssertNil(TypedClaimedProfilesDecoder.decode(from: []))
    }

    func testWrongSchemaClaimedProfilesFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedClaimedProfilesDecoder.key,
            schemaId: "not.claimed_profiles",
            schemaVersion: 1,
            fileIdentifier: TypedClaimedProfilesDecoder.fileIdentifier,
            payload: buildProfileMap(buildClaimedProfiles, [("pk", .simple(pubkey: "pk", display: "x"))]))
        XCTAssertNil(TypedClaimedProfilesDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    /// An empty claimed-profile map (fresh kernel, no claims) must decode to an
    /// EMPTY dictionary, NOT nil — nil would wrongly trigger the JSON fallback
    /// when the typed path is in fact authoritative.
    func testEmptyClaimedProfilesBufferDecodesToEmptyMap() throws {
        let map = try XCTUnwrap(
            TypedClaimedProfilesDecoder.decode(bytes: buildProfileMap(buildClaimedProfiles, [])))
        XCTAssertTrue(map.isEmpty)
    }

    // MARK: - resolved_profiles (KRPR)

    func testResolvedProfilesSidecarIdentityIsExact() {
        XCTAssertEqual(TypedResolvedProfilesDecoder.key, "resolved_profiles")
        XCTAssertEqual(TypedResolvedProfilesDecoder.schemaId, "resolved_profiles")
        XCTAssertEqual(TypedResolvedProfilesDecoder.fileIdentifier, "KRPR")
    }

    func testTypedResolvedProfilesSidecarDecodes() throws {
        let a = CardFields.simple(pubkey: "pk1", display: "Carol")
        let envelope = TypedProjectionEnvelope(
            key: TypedResolvedProfilesDecoder.key,
            schemaId: TypedResolvedProfilesDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedResolvedProfilesDecoder.fileIdentifier,
            payload: buildProfileMap(buildResolvedProfiles, [("pk1", a)]))

        let map = try XCTUnwrap(TypedResolvedProfilesDecoder.decode(from: [envelope]))
        XCTAssertEqual(map["pk1"]?.displayName, "Carol")
    }

    func testAbsentResolvedProfilesSidecarFallsBack() {
        XCTAssertNil(TypedResolvedProfilesDecoder.decode(from: []))
    }

    func testEmptyResolvedProfilesBufferDecodesToEmptyMap() throws {
        let map = try XCTUnwrap(
            TypedResolvedProfilesDecoder.decode(bytes: buildProfileMap(buildResolvedProfiles, [])))
        XCTAssertTrue(map.isEmpty)
    }

    // MARK: - shared ProfileCard wire round-trip

    /// The SAME `nmp_kernel_ProfileCard` reader must decode identically whether
    /// it arrives via the `profile` single-card root or a `claimed_profiles` map
    /// entry — proving the shared-binding dedup did not change the wire.
    func testSharedProfileCardDecodesIdenticallyAcrossKeys() throws {
        let single = try XCTUnwrap(TypedProfileDecoder.decode(bytes: buildProfile(Self.fullCard)))
        let mapped = try XCTUnwrap(
            TypedClaimedProfilesDecoder.decode(
                bytes: buildProfileMap(buildClaimedProfiles, [(Self.fullCard.pubkey, Self.fullCard)])))
        XCTAssertEqual(single, mapped[Self.fullCard.pubkey])
    }

    // MARK: - builders

    /// Plain value mirror of the `ProfileCard` wire fields for buffer building.
    struct CardFields {
        let pubkey: String
        let displayName: String?
        let pictureUrl: String?
        let nip05: String
        let about: String
        let lnurl: String?

        static func simple(pubkey: String, display: String) -> CardFields {
            CardFields(
                pubkey: pubkey, displayName: display,
                pictureUrl: nil, nip05: "", about: "", lnurl: nil)
        }
    }

    private func assertEqual(_ card: ProfileCard, _ fields: CardFields,
                             file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(card.pubkey, fields.pubkey, file: file, line: line)
        XCTAssertEqual(card.displayName, fields.displayName, file: file, line: line)
        XCTAssertEqual(card.pictureUrl, fields.pictureUrl, file: file, line: line)
        XCTAssertEqual(card.nip05, fields.nip05, file: file, line: line)
        XCTAssertEqual(card.about, fields.about, file: file, line: line)
        XCTAssertEqual(card.lnurl, fields.lnurl, file: file, line: line)
    }

    /// Encode one `ProfileCard` into `fbb`, returning its offset. Mirrors the
    /// Rust producer's `has_*`/value encoding.
    private func encodeCard(_ fbb: inout FlatBufferBuilder, _ c: CardFields) -> Offset {
        let pubkeyOff = fbb.create(string: c.pubkey)
        let displayOff = c.displayName.map { fbb.create(string: $0) } ?? Offset()
        let pictureOff = c.pictureUrl.map { fbb.create(string: $0) } ?? Offset()
        let nip05Off = fbb.create(string: c.nip05)
        let aboutOff = fbb.create(string: c.about)
        let lnurlOff = c.lnurl.map { fbb.create(string: $0) } ?? Offset()
        return nmp_kernel_ProfileCard.createProfileCard(
            &fbb,
            pubkeyOffset: pubkeyOff,
            hasDisplayName: c.displayName != nil,
            displayNameOffset: displayOff,
            hasPictureUrl: c.pictureUrl != nil,
            pictureUrlOffset: pictureOff,
            nip05Offset: nip05Off,
            aboutOffset: aboutOff,
            hasLnurl: c.lnurl != nil,
            lnurlOffset: lnurlOff)
    }

    private func buildProfile(_ c: CardFields) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let cardOff = encodeCard(&fbb, c)
        let root = nmp_kernel_ProfileSnapshot.createProfileSnapshot(&fbb, cardOffset: cardOff)
        nmp_kernel_ProfileSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildClaimedProfiles(_ entries: [(String, CardFields)]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let rows: [Offset] = entries.map { (key, card) in
            let keyOff = fbb.create(string: key)
            let cardOff = encodeCard(&fbb, card)
            return nmp_kernel_ClaimedProfileEntry.createClaimedProfileEntry(
                &fbb, keyOffset: keyOff, valueOffset: cardOff)
        }
        let vec = fbb.createVector(ofOffsets: rows)
        let root = nmp_kernel_ClaimedProfilesSnapshot.createClaimedProfilesSnapshot(
            &fbb, entriesVectorOffset: vec)
        nmp_kernel_ClaimedProfilesSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildResolvedProfiles(_ entries: [(String, CardFields)]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let rows: [Offset] = entries.map { (key, card) in
            let keyOff = fbb.create(string: key)
            let cardOff = encodeCard(&fbb, card)
            return nmp_kernel_ResolvedProfileEntry.createResolvedProfileEntry(
                &fbb, keyOffset: keyOff, valueOffset: cardOff)
        }
        let vec = fbb.createVector(ofOffsets: rows)
        let root = nmp_kernel_ResolvedProfilesSnapshot.createResolvedProfilesSnapshot(
            &fbb, entriesVectorOffset: vec)
        nmp_kernel_ResolvedProfilesSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildProfileMap(
        _ builder: ([(String, CardFields)]) -> Data,
        _ entries: [(String, CardFields)]
    ) -> Data {
        builder(entries)
    }
}
