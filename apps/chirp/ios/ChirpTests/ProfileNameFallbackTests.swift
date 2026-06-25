import XCTest
@testable import Chirp

/// Unit coverage for the profile-name flicker defect (Chirp iOS).
///
/// ## The defect
///
/// When the user navigates away from the timeline and back, a profile card may
/// be absent for 1–2 snapshot ticks (~250–500ms) even though the kernel still
/// has the kind:0 cached. During that window `KernelModel.profile(forPubkey:)`
/// returns `nil` and `NoteRowView.authorDisplayLabel` falls through to
/// `pubkey.shortHex`, so a real name briefly flickers to a raw hex stub.
///
/// ADR-0063 Lane H (#1671): `claimed_profiles` (KCPR) and `resolved_profiles`
/// (KRPR) JSON snapshot projections are deleted. Profile data is now served via
/// the `refs.profile` KPRF NRRD row-delta sidecar. These tests exercise the
/// SAME read path (`keyedRefCache` → `profileCard(forPubkey:)`) via the
/// `setTypedSnapshotForTesting(profileCards:)` test seam.
///
///   * Test A — `KernelModel.profile(forPubkey:)` fallback + the `isRawKey`
///     guard that stops a no-kind:0 card from echoing the raw key as a name.
///   * Test B — `NoteRowView`'s `eventCards` gap-filler, the rung that keeps
///     a row labelled during a profile-miss window.
@MainActor
final class ProfileNameFallbackTests: XCTestCase {

    /// 64-char hex pubkey under test. `shortHex` → `"deadbeef…deadbeef"`.
    private let pk = String(repeating: "deadbeef", count: 8)

    // MARK: - Synthetic snapshot construction

    /// ADR-0063 Lane H: the old `ProfileFixture(claimed:resolved:)` pair is gone.
    /// Tests now supply the merged per-key profile map directly to
    /// `setTypedSnapshotForTesting(profileCards:)`.

    /// A profile `ProfileCard`. `displayName` is `nil` to model "no kind:0 yet"
    /// (the card's `displayLabel` then falls back to the abbreviated hex pubkey).
    private func card(pubkey: String, displayName: String?) -> ProfileCard {
        ProfileCard(
            pubkey: pubkey,
            displayName: displayName,
            name: nil,
            rawDisplayName: nil,
            displayNameCamel: nil,
            pictureUrl: nil,
            banner: nil,
            website: nil,
            nip05: "",
            about: "",
            lud16: nil,
            lud06: nil,
            lnurl: nil)
    }

    private func model(cards: [String: ProfileCard] = [:]) -> KernelModel {
        let m = KernelModel()
        m.setTypedSnapshotForTesting(profileCards: cards)
        return m
    }

    // MARK: - Test A — profile(forPubkey:) fallback chain

    func test_profile_forPubkey_fallback_chain() throws {
        // 1. Profile card with a real display name → returned verbatim.
        XCTAssertEqual(
            model(cards: [pk: card(pubkey: pk, displayName: "Alice")]).profile(forPubkey: pk)?.display, "Alice",
            "A profile card with a non-empty displayName must win.")

        // 2. A non-shortHex display from the resolved set → display is returned.
        XCTAssertEqual(
            model(cards: [pk: card(pubkey: pk, displayName: "Bob")]).profile(forPubkey: pk)?.display, "Bob",
            "A resolved display name must be returned from the profile card.")

        // 3. No kind:0 yet (displayName == nil → card's displayLabel falls back to
        //    shortHex). The `isRawKey` guard must blank displayName so the row does
        //    NOT echo a raw key as if it were a real name.
        let rawProfile = model(cards: [pk: card(pubkey: pk, displayName: nil)]).profile(forPubkey: pk)
        XCTAssertNotNil(rawProfile, "A card with nil displayName still yields a ProfileWire.")
        XCTAssertNil(
            rawProfile?.displayName,
            "isRawKey guard must nil out displayName when card.displayLabel == shortHex.")

        // 4. No cards → profile(forPubkey:) is nil → the caller is responsible
        //    for showing shortHex.
        XCTAssertNil(
            model().profile(forPubkey: pk),
            "With no profile data the accessor must return nil (caller → shortHex).")
    }

    func test_nameRegressionMetric_counts_only_missing_after_resolved_name() throws {
        let aliceCards: [String: ProfileCard] = [pk: card(pubkey: pk, displayName: "Alice")]
        let m = model()

        XCTAssertNil(m.profile(forPubkey: pk))
        XCTAssertEqual(
            m.appMetrics.nameRegressionCount, 0,
            "First-load misses must not be counted as name regressions.")

        m.setTypedSnapshotForTesting(profileCards: aliceCards)
        XCTAssertEqual(m.profile(forPubkey: pk)?.display, "Alice")
        XCTAssertEqual(
            m.appMetrics.nameRegressionCount, 0,
            "Resolving a name arms the detector without incrementing it.")

        m.setTypedSnapshotForTesting(profileCards: [:])
        XCTAssertNil(m.profile(forPubkey: pk))
        XCTAssertEqual(
            m.appMetrics.nameRegressionCount, 1,
            "The first missing profile after a resolved name is a regression.")

        XCTAssertNil(m.profile(forPubkey: pk))
        XCTAssertEqual(
            m.appMetrics.nameRegressionCount, 1,
            "Repeated reads during the same missing window must not overcount.")

        m.setTypedSnapshotForTesting(profileCards: aliceCards)
        XCTAssertEqual(m.profile(forPubkey: pk)?.display, "Alice")
        m.setTypedSnapshotForTesting(profileCards: [:])
        XCTAssertNil(m.profile(forPubkey: pk))
        XCTAssertEqual(
            m.appMetrics.nameRegressionCount, 2,
            "A resolved name re-arms the detector for a later regression.")
    }

    // MARK: - Test B — NoteRowView author-label gap filler

    /// Locks the `eventCards` rung of `NoteRowView.resolveAuthorLabel` as
    /// load-bearing. During the claim-churn window `profileDisplay` is `nil`;
    /// the event-card author name (NOFS gap-filler, NoteRowView:45) is what
    /// keeps the row labelled instead of collapsing to `shortHex`.
    ///
    /// `authorDisplayLabel` itself is a `private` computed property reading an
    /// `@EnvironmentObject`, which XCTest cannot exercise; the pure
    /// `resolveAuthorLabel` helper it delegates to is the testable seam.
    func test_noteRow_authorDisplayLabel_eventCards_gap_filler() {
        let short = pk.shortHex

        // the refs.profile resolve dropped this pubkey (profileDisplay == nil), but
        // the event card still carries the author name → that name must show.
        XCTAssertEqual(
            NoteRowView.resolveAuthorLabel(
                profileDisplay: nil,
                eventCardName: "Carol",
                shortHex: short),
            "Carol",
            "eventCards author name must fill the gap when the profile claim churns.")

        // Precedence: a live profile display still outranks the event card.
        XCTAssertEqual(
            NoteRowView.resolveAuthorLabel(
                profileDisplay: "Alice",
                eventCardName: "Carol",
                shortHex: short),
            "Alice",
            "A resolved profile display must outrank the event-card gap-filler.")

        // PR #823: the snapshot-baked itemAuthorName fills the gap when the
        // claim churns, BEFORE the event-card rung. Claim-independent — this
        // is the structural flicker fix.
        XCTAssertEqual(
            NoteRowView.resolveAuthorLabel(
                profileDisplay: nil,
                itemAuthorName: "Bob",
                eventCardName: "Carol",
                shortHex: short),
            "Bob",
            "The typed-card author name must outrank the event-card fallback and prevent the flicker.")

        // Full collapse: nothing resolves → shortHex is the last resort.
        XCTAssertEqual(
            NoteRowView.resolveAuthorLabel(
                profileDisplay: nil,
                eventCardName: nil,
                shortHex: short),
            short,
            "With no name source the label collapses to shortHex.")
    }

    // MARK: - Test C — DM peer labels and compose search

    func test_dmPeerPresentation_prefersResolvedProfileDisplay() {
        XCTAssertEqual(
            DmPeerPresentation.label(pubkey: pk, profileDisplay: "Alice"),
            "Alice",
            "DM peer labels must prefer the Rust-owned resolved profile display.")
        XCTAssertEqual(
            DmPeerPresentation.label(pubkey: pk, profileDisplay: nil),
            pk.shortHex,
            "Missing profile data must fall back to the existing presentation short key.")
        XCTAssertEqual(
            DmPeerPresentation.label(pubkey: pk, profileDisplay: ""),
            pk.shortHex,
            "Empty profile labels must not render blank DM peers.")
    }

    func test_dmComposeSearch_matchesResolvedDisplayOrRawPubkey() {
        XCTAssertTrue(
            DmPeerPresentation.matchesContact(
                pubkey: pk, profileDisplay: "Alice", query: "ali"),
            "Compose search must find contacts by resolved display name.")
        XCTAssertTrue(
            DmPeerPresentation.matchesContact(
                pubkey: pk, profileDisplay: "Alice", query: String(pk.prefix(8))),
            "Compose search must still find contacts by raw pubkey.")
        XCTAssertTrue(
            DmPeerPresentation.matchesContact(
                pubkey: pk, profileDisplay: "Alice", query: "   "),
            "An empty trimmed query must show all contacts.")
        XCTAssertFalse(
            DmPeerPresentation.matchesContact(
                pubkey: pk, profileDisplay: "Alice", query: "carol"),
            "Unmatched display and pubkey text must be filtered out.")
    }

}
