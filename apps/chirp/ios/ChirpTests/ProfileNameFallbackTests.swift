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
/// REAL read path (`.convertFromSnakeCase` decode → `snapshot` slot →
/// projection accessors) so a CodingKey drift on `claimed_profiles` /
/// `resolved_profiles` fails loudly here:
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

    /// The exact decoder configuration `KernelHandle` uses for the kernel
    /// snapshot payload — `.convertFromSnakeCase`, so the kernel's snake_case
    /// JSON keys map onto the Swift property names. Reproduced bit-for-bit so
    /// a CodingKey-mapping drift surfaces here, not silently in production.
    private func snapshotDecoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }

    /// Decode a minimal but schema-valid `KernelUpdate` carrying the two
    /// profile projections under test. The `metrics` blob is the only verbose
    /// part — every `KernelMetrics` field is required by the synthesized
    /// decoder, so the zeros are written once here and reused by every test.
    /// If `KernelMetrics` grows a required field this helper fails to decode,
    /// which is the same schema-drift-surfacing behaviour the conformance
    /// suite relies on — fix the template, do not route around it.
    ///
    /// - Parameters:
    ///   - claimed: pubkey → `claimed_profiles` card JSON object body.
    ///   - resolved: pubkey → `resolved_profiles` card JSON object body
    ///     (drives `KernelModel.mentionProfiles`).
    private func makeKernelUpdate(
        claimed: [String: String] = [:],
        resolved: [String: String] = [:]
    ) throws -> KernelUpdate {
        func mapJSON(_ entries: [String: String]) -> String {
            entries.map { "\"\($0.key)\": \($0.value)" }.joined(separator: ",\n")
        }

        let json = """
        {
          "rev": 1,
          "schema_version": 1,
          "running": true,
          "relay_statuses": [],
          "metrics": \(Self.metricsZerosJSON),
          "projections": {
            "claimed_profiles": { \(mapJSON(claimed)) },
            "resolved_profiles": { \(mapJSON(resolved)) }
          }
        }
        """
        return try snapshotDecoder().decode(KernelUpdate.self, from: Data(json.utf8))
    }

    /// A `claimed_profiles` / `resolved_profiles` card body. `displayName` is
    /// passed pre-quoted-or-null so callers can model "no kind:0 yet".
    private func cardJSON(pubkey: String, displayName: String?) -> String {
        let nameField = displayName.map { "\"\($0)\"" } ?? "null"
        return """
        {
          "pubkey": "\(pubkey)",
          "npub": "npub1\(String(repeating: "q", count: 58))",
          "display_name": \(nameField),
          "picture_url": null,
          "nip05": "",
          "about": "",
          "has_profile": \(displayName != nil)
        }
        """
    }

    private func model(with update: KernelUpdate) -> KernelModel {
        let m = KernelModel()
        m.setSnapshotForTesting(update)
        return m
    }

    // MARK: - Test A — profile(forPubkey:) fallback chain

    func test_profile_forPubkey_fallback_chain() throws {
        // 1. claimed_profiles carries a real display name → returned verbatim.
        let claimedUpdate = try makeKernelUpdate(
            claimed: [pk: cardJSON(pubkey: pk, displayName: "Alice")])
        XCTAssertEqual(
            model(with: claimedUpdate).profile(forPubkey: pk)?.display, "Alice",
            "A claimed_profiles card with a non-empty displayName must win.")

        // 2. claimed_profiles empty, resolved_profiles (mentionProfiles) carries
        //    a real, non-shortHex display → mention display is returned.
        let mentionUpdate = try makeKernelUpdate(
            resolved: [pk: cardJSON(pubkey: pk, displayName: "Bob")])
        XCTAssertEqual(
            model(with: mentionUpdate).profile(forPubkey: pk)?.display, "Bob",
            "With no claimed card, the resolved/mention display must fill in.")

        // 3. mention display == shortHex (no kind:0 → ProfileCard.displayLabel
        //    falls back to shortHex, so MentionProfile.display == shortHex).
        //    The `isRawKey` guard must blank displayName so the row does NOT
        //    echo a raw key as if it were a real name.
        let rawKeyUpdate = try makeKernelUpdate(
            resolved: [pk: cardJSON(pubkey: pk, displayName: nil)])
        let rawProfile = model(with: rawKeyUpdate).profile(forPubkey: pk)
        XCTAssertNotNil(rawProfile, "A mention card still yields a ProfileWire.")
        XCTAssertNil(
            rawProfile?.displayName,
            "isRawKey guard must nil out displayName when mention.display == shortHex.")

        // 4. Both projections empty → profile(forPubkey:) is nil → the caller
        //    is responsible for showing shortHex.
        let emptyUpdate = try makeKernelUpdate()
        XCTAssertNil(
            model(with: emptyUpdate).profile(forPubkey: pk),
            "With no profile data the accessor must return nil (caller → shortHex).")
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
                mentionDisplay: nil,
                shortHex: short),
            "Carol",
            "eventCards author name must fill the gap when the profile claim churns.")

        // Precedence: a live profile display still outranks the event card.
        XCTAssertEqual(
            NoteRowView.resolveAuthorLabel(
                profileDisplay: "Alice",
                eventCardName: "Carol",
                mentionDisplay: nil,
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
                mentionDisplay: nil,
                shortHex: short),
            "Bob",
            "The TimelineItem-baked author name must outrank the event card and prevent the flicker.")

        // Full collapse: nothing resolves → shortHex is the last resort.
        XCTAssertEqual(
            NoteRowView.resolveAuthorLabel(
                profileDisplay: nil,
                eventCardName: nil,
                mentionDisplay: nil,
                shortHex: short),
            short,
            "With no name source the label collapses to shortHex.")
    }

    // MARK: - Fixtures

    /// Every `KernelMetrics` field zeroed (optionals null). Written once;
    /// reused by `makeKernelUpdate`. Snake_case keys — the decoder applies
    /// `.convertFromSnakeCase` on the way in.
    private static let metricsZerosJSON = """
    {
      "actor_queue_depth": 0,
      "bytes_rx": 0,
      "bytes_tx": 0,
      "claim_drops_total": 0,
      "closed_rx": 0,
      "contacts_authors": 0,
      "delete_events": 0,
      "diagnostic_firehose_events": 0,
      "dispatch_drops_total": 0,
      "duplicate_events": 0,
      "emit_hz_configured": 0,
      "eose_rx": 0,
      "estimated_store_bytes": 0,
      "events_per_second_configured": 0,
      "events_rx": 0,
      "events_since_last_update": 0,
      "first_event_ms": null,
      "frames_rx": 0,
      "generated_events": 0,
      "inserted_count": 0,
      "last_event_to_emit_ms": null,
      "make_update_us": 0,
      "max_event_to_emit_ms": 0,
      "max_events_per_update": 0,
      "note_events": 0,
      "notices_rx": 0,
      "open_views": 0,
      "payload_bytes": 0,
      "profile_events": 0,
      "removed_count": 0,
      "serialize_us": 0,
      "store_to_payload_ratio": 0.0,
      "stored_events": 0,
      "target_profile_loaded_ms": null,
      "timeline_authors": 0,
      "timeline_first_item_ms": null,
      "timeline_opened_ms": null,
      "tombstones": 0,
      "update_emitted_ms": null,
      "update_frame_degradations_total": 0,
      "update_sequence": 0,
      "updated_count": 0,
      "visible_items": 0,
      "visible_placeholder_avatar_items": 0,
      "visible_profiled_items": 0
    }
    """
}
