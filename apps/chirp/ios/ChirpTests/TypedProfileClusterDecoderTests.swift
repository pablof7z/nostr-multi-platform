import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the profile sidecar: `profile` (`KPRF`).
///
/// ADR-0063 Lane H (#1671): `claimed_profiles` (KCPR) and `resolved_profiles`
/// (KRPR) JSON snapshot projections are deleted. The KCPR/KRPR decoder tests
/// and the `testSharedProfileCardDecodesIdenticallyAcrossKeys` cross-key test
/// that depended on them are removed. The KPRF tests remain.
///
/// These mirror `TypedAppProjectionsDecoderTests`: build the typed FlatBuffers
/// buffer directly via the generated builders, wrap it in a
/// `TypedProjectionEnvelope` carrying the producer's `(key, schemaId)`, and
/// assert the generated `TypedProfileDecoder` produces the Chirp domain value.
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
        name: "typed",
        rawDisplayName: "Typed Display",
        displayNameCamel: "Typed Camel",
        pictureUrl: "https://typed.example/pic.png",
        banner: "https://typed.example/banner.png",
        website: "https://typed.example",
        nip05: "typed@example.com",
        about: "typed about text",
        lud16: "typed@ln.example",
        lud06: "lnurl1typed",
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
            schemaVersion: 2,
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
            schemaVersion: 2,
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

    // ADR-0063 Lane H: KCPR / KRPR test sections deleted.
    // `testClaimedProfilesSidecarIdentityIsExact`, `testTypedClaimedProfilesSidecarDecodes`,
    // `testAbsentClaimedProfilesSidecarFallsBack`, `testWrongSchemaClaimedProfilesFallsBack`,
    // `testEmptyClaimedProfilesBufferDecodesToEmptyMap`, `testResolvedProfilesSidecarIdentityIsExact`,
    // `testTypedResolvedProfilesSidecarDecodes`, `testAbsentResolvedProfilesSidecarFallsBack`,
    // `testEmptyResolvedProfilesBufferDecodesToEmptyMap`, and
    // `testSharedProfileCardDecodesIdenticallyAcrossKeys` all deleted.

    // MARK: - builders

    /// Plain value mirror of the `ProfileCard` wire fields for buffer building.
    struct CardFields {
        let pubkey: String
        let displayName: String?
        let name: String?
        let rawDisplayName: String?
        let displayNameCamel: String?
        let pictureUrl: String?
        let banner: String?
        let website: String?
        let nip05: String
        let about: String
        let lud16: String?
        let lud06: String?
        let lnurl: String?

        init(pubkey: String,
             displayName: String?,
             name: String? = nil,
             rawDisplayName: String? = nil,
             displayNameCamel: String? = nil,
             pictureUrl: String?,
             banner: String? = nil,
             website: String? = nil,
             nip05: String,
             about: String,
             lud16: String? = nil,
             lud06: String? = nil,
             lnurl: String?) {
            self.pubkey = pubkey
            self.displayName = displayName
            self.name = name
            self.rawDisplayName = rawDisplayName
            self.displayNameCamel = displayNameCamel
            self.pictureUrl = pictureUrl
            self.banner = banner
            self.website = website
            self.nip05 = nip05
            self.about = about
            self.lud16 = lud16
            self.lud06 = lud06
            self.lnurl = lnurl
        }

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
        XCTAssertEqual(card.name, fields.name, file: file, line: line)
        XCTAssertEqual(card.rawDisplayName, fields.rawDisplayName, file: file, line: line)
        XCTAssertEqual(card.displayNameCamel, fields.displayNameCamel, file: file, line: line)
        XCTAssertEqual(card.pictureUrl, fields.pictureUrl, file: file, line: line)
        XCTAssertEqual(card.banner, fields.banner, file: file, line: line)
        XCTAssertEqual(card.website, fields.website, file: file, line: line)
        XCTAssertEqual(card.nip05, fields.nip05, file: file, line: line)
        XCTAssertEqual(card.about, fields.about, file: file, line: line)
        XCTAssertEqual(card.lud16, fields.lud16, file: file, line: line)
        XCTAssertEqual(card.lud06, fields.lud06, file: file, line: line)
        XCTAssertEqual(card.lnurl, fields.lnurl, file: file, line: line)
    }

    /// Encode one `ProfileCard` into `fbb`, returning its offset. Mirrors the
    /// Rust producer's `has_*`/value encoding.
    private func encodeCard(_ fbb: inout FlatBufferBuilder, _ c: CardFields) -> Offset {
        let pubkeyOff = fbb.create(string: c.pubkey)
        let displayOff = c.displayName.map { fbb.create(string: $0) } ?? Offset()
        let nameOff = c.name.map { fbb.create(string: $0) } ?? Offset()
        let rawDisplayOff = c.rawDisplayName.map { fbb.create(string: $0) } ?? Offset()
        let displayCamelOff = c.displayNameCamel.map { fbb.create(string: $0) } ?? Offset()
        let pictureOff = c.pictureUrl.map { fbb.create(string: $0) } ?? Offset()
        let bannerOff = c.banner.map { fbb.create(string: $0) } ?? Offset()
        let websiteOff = c.website.map { fbb.create(string: $0) } ?? Offset()
        let nip05Off = fbb.create(string: c.nip05)
        let aboutOff = fbb.create(string: c.about)
        let lud16Off = c.lud16.map { fbb.create(string: $0) } ?? Offset()
        let lud06Off = c.lud06.map { fbb.create(string: $0) } ?? Offset()
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
            lnurlOffset: lnurlOff,
            hasName: c.name != nil,
            nameOffset: nameOff,
            hasRawDisplayName: c.rawDisplayName != nil,
            rawDisplayNameOffset: rawDisplayOff,
            hasDisplayNameCamel: c.displayNameCamel != nil,
            displayNameCamelOffset: displayCamelOff,
            hasBanner: c.banner != nil,
            bannerOffset: bannerOff,
            hasWebsite: c.website != nil,
            websiteOffset: websiteOff,
            hasLud16: c.lud16 != nil,
            lud16Offset: lud16Off,
            hasLud06: c.lud06 != nil,
            lud06Offset: lud06Off)
    }

    private func buildProfile(_ c: CardFields) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let cardOff = encodeCard(&fbb, c)
        let root = nmp_kernel_ProfileSnapshot.createProfileSnapshot(&fbb, cardOffset: cardOff)
        nmp_kernel_ProfileSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    // ADR-0063 Lane H: buildClaimedProfiles / buildResolvedProfiles / buildProfileMap
    // helpers deleted (used KCPR/KRPR types that no longer exist).
}
