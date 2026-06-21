import XCTest
@testable import Chirp

/// Unit coverage for the profile-name flicker defect (Chirp iOS).
///
/// ## The defect
///
/// When the user navigates away from the timeline and back,
/// `claimed_profiles[pubkey]` is absent for 1–2 snapshot ticks (~250–500ms)
/// even though the kernel still has the kind:0 cached. During that window
/// `KernelModel.profile(forPubkey:)` returns `nil` and
/// `NoteRowView.authorDisplayLabel` falls through to `pubkey.shortHex`, so a
/// real name briefly flickers to a raw hex stub. This is a Swift-side
/// claim-churn gap, not a kernel data loss.
///
/// These tests lock the two load-bearing fallback behaviours that keep the
/// flicker from being worse than a single regression rung, exercised on the
/// REAL read path (typed `claimedProfiles` / `resolvedProfiles` slots →
/// projection accessors — the SAME slots `apply(result:)` assigns from the
/// `KCPR` / `KRPR` sidecars):
///
///   * Test A — `KernelModel.profile(forPubkey:)` precedence + the `isRawKey`
///     guard that stops a mention card from echoing the raw key as a name.
///   * Test B — `NoteRowView`'s `eventCards` gap-filler, the rung that keeps
///     a row labelled during the claim-churn window.
@MainActor
final class ProfileNameFallbackTests: XCTestCase {

    /// 64-char hex pubkey under test. `shortHex` → `"deadbeef…deadbeef"`.
    private let pk = String(repeating: "deadbeef", count: 8)

    // MARK: - Synthetic snapshot construction

    /// A bundle of the two profile-cluster inputs under test, fed to
    /// `KernelModel.setTypedSnapshotForTesting`, which seeds the per-key profile
    /// override that `profileCard(forPubkey:)` reads — the SAME read path the
    /// live shell uses (`keyedRefCache` → `profileCard(forPubkey:)`), ADR-0063
    /// Lane E (#1671). `claimed` wins over `resolved` for the same pubkey,
    /// mirroring the kernel's projection precedence.
    private struct ProfileFixture {
        let claimed: [String: ProfileCard]
        let resolved: [String: ProfileCard]
    }

    /// Build the two typed profile projections under test directly as
    /// `ProfileCard` maps — the typed read path. The generic `payload:Value`
    /// JSON decode is retired (Chirp no longer reads it); the typed sidecars
    /// are authoritative, so the fixture exercises the authoritative path.
    ///
    /// - Parameters:
    ///   - claimed: pubkey → claimed-profile card (wins on conflict).
    ///   - resolved: pubkey → resolved/mention-profile card.
    private func makeProfileFixture(
        claimed: [String: ProfileCard] = [:],
        resolved: [String: ProfileCard] = [:]
    ) -> ProfileFixture {
        ProfileFixture(claimed: claimed, resolved: resolved)
    }

    /// A `claimed_profiles` / `resolved_profiles` `ProfileCard`. `displayName`
    /// is `nil` to model "no kind:0 yet" (the card's `displayLabel` then falls
    /// back to the abbreviated hex pubkey).
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

    private func model(with fixture: ProfileFixture) -> KernelModel {
        let m = KernelModel()
        m.setTypedSnapshotForTesting(
            claimedProfiles: fixture.claimed,
            resolvedProfiles: fixture.resolved)
        return m
    }

    // MARK: - Test A — profile(forPubkey:) fallback chain

    func test_profile_forPubkey_fallback_chain() throws {
        // 1. claimed_profiles carries a real display name → returned verbatim.
        let claimedFixture = makeProfileFixture(
            claimed: [pk: card(pubkey: pk, displayName: "Alice")])
        XCTAssertEqual(
            model(with: claimedFixture).profile(forPubkey: pk)?.display, "Alice",
            "A claimed_profiles card with a non-empty displayName must win.")

        // 2. claimed_profiles empty, resolved_profiles (mentionProfiles) carries
        //    a real, non-shortHex display → mention display is returned.
        let mentionFixture = makeProfileFixture(
            resolved: [pk: card(pubkey: pk, displayName: "Bob")])
        XCTAssertEqual(
            model(with: mentionFixture).profile(forPubkey: pk)?.display, "Bob",
            "With no claimed card, the resolved/mention display must fill in.")

        // 3. mention display == shortHex (no kind:0 → ProfileCard.displayLabel
        //    falls back to shortHex, so MentionProfile.display == shortHex).
        //    The `isRawKey` guard must blank displayName so the row does NOT
        //    echo a raw key as if it were a real name.
        let rawKeyFixture = makeProfileFixture(
            resolved: [pk: card(pubkey: pk, displayName: nil)])
        let rawProfile = model(with: rawKeyFixture).profile(forPubkey: pk)
        XCTAssertNotNil(rawProfile, "A mention card still yields a ProfileWire.")
        XCTAssertNil(
            rawProfile?.displayName,
            "isRawKey guard must nil out displayName when mention.display == shortHex.")

        // 4. Both projections empty → profile(forPubkey:) is nil → the caller
        //    is responsible for showing shortHex.
        let emptyFixture = makeProfileFixture()
        XCTAssertNil(
            model(with: emptyFixture).profile(forPubkey: pk),
            "With no profile data the accessor must return nil (caller → shortHex).")
    }

    func test_nameRegressionMetric_counts_only_missing_after_resolved_name() throws {
        let claimed = makeProfileFixture(
            claimed: [pk: card(pubkey: pk, displayName: "Alice")])
        let empty = makeProfileFixture()
        let m = model(with: empty)

        XCTAssertNil(m.profile(forPubkey: pk))
        XCTAssertEqual(
            m.appMetrics.nameRegressionCount, 0,
            "First-load misses must not be counted as name regressions.")

        m.setTypedSnapshotForTesting(claimedProfiles: claimed.claimed, resolvedProfiles: claimed.resolved)
        XCTAssertEqual(m.profile(forPubkey: pk)?.display, "Alice")
        XCTAssertEqual(
            m.appMetrics.nameRegressionCount, 0,
            "Resolving a name arms the detector without incrementing it.")

        m.setTypedSnapshotForTesting(claimedProfiles: empty.claimed, resolvedProfiles: empty.resolved)
        XCTAssertNil(m.profile(forPubkey: pk))
        XCTAssertEqual(
            m.appMetrics.nameRegressionCount, 1,
            "The first missing profile after a resolved name is a regression.")

        XCTAssertNil(m.profile(forPubkey: pk))
        XCTAssertEqual(
            m.appMetrics.nameRegressionCount, 1,
            "Repeated reads during the same missing window must not overcount.")

        m.setTypedSnapshotForTesting(claimedProfiles: claimed.claimed, resolvedProfiles: claimed.resolved)
        XCTAssertEqual(m.profile(forPubkey: pk)?.display, "Alice")
        m.setTypedSnapshotForTesting(claimedProfiles: empty.claimed, resolvedProfiles: empty.resolved)
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

        // claimed_profiles dropped this pubkey (profileDisplay == nil), but the
        // event card still carries the author name → that name must show.
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
            "The TimelineItem-baked author name must outrank the event card and prevent the flicker.")

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
