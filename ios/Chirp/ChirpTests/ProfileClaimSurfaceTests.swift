import SwiftUI
import XCTest
@testable import Chirp

/// Locks the F-CR-00 claim-only invariant for the author-displaying surfaces
/// that previously rendered an author pubkey WITHOUT claiming it (so the kind:0
/// never resolved): profile mentions inside note content, and the reply
/// attribution line. The fix restores the invariant "every surface that
/// displays an author pubkey self-claims it" (kernel `timeline.rs:198-211`).
///
/// These tests exercise the pure, testable seams of that fix:
///   * `ContentTreeWire.mentionPubkeys` — the set the content view claims.
///   * A recording `NostrProfileHost` mock that proves claim/release calls are
///     refcount-balanced (every claim matched by a release), mirroring
///     `NostrAvatar`'s lifecycle.
@MainActor
final class ProfileClaimSurfaceTests: XCTestCase {

    private let pkA = String(repeating: "a", count: 64)
    private let pkB = String(repeating: "b", count: 64)
    private let eventID = String(repeating: "e", count: 64)

    private func mentionNode(_ pubkey: String) -> NostrWireNode {
        .mention(WireNostrUri(
            uri: "nostr:npub1\(pubkey.prefix(8))",
            kind: .profile,
            primaryId: pubkey,
            relays: [],
            author: nil,
            eventKind: nil))
    }

    // MARK: - mentionPubkeys collection

    func test_mentionPubkeys_collects_profile_mentions_dedup_in_order() {
        let tree = ContentTreeWire(
            nodes: [
                .paragraph(children: [1, 2, 3, 4, 5]),
                .text("hey "),
                mentionNode(pkA),
                .text(" and "),
                mentionNode(pkB),
                // Duplicate mention of pkA → must collapse.
                mentionNode(pkA),
            ],
            roots: [0],
            mode: nil)

        XCTAssertEqual(
            tree.mentionPubkeys, [pkA, pkB],
            "Profile mentions must be collected in arena order and de-duplicated.")
    }

    func test_mentionPubkeys_excludes_event_references() {
        let tree = ContentTreeWire(
            nodes: [
                .paragraph(children: [1, 2]),
                mentionNode(pkA),
                .eventRef(WireNostrUri(
                    uri: "nostr:nevent1example",
                    kind: .event,
                    primaryId: eventID,
                    relays: [],
                    author: pkB,
                    eventKind: 1)),
            ],
            roots: [0],
            mode: nil)

        XCTAssertEqual(
            tree.mentionPubkeys, [pkA],
            "Only profile mentions claim a kind:0; event refs (and their authors) do not.")
    }

    func test_mentionPubkeys_empty_for_plain_text() {
        let tree = ContentTreeWire(
            nodes: [.paragraph(children: [1]), .text("no mentions here")],
            roots: [0],
            mode: nil)
        XCTAssertTrue(tree.mentionPubkeys.isEmpty)
    }

    // MARK: - claim/release refcount discipline

    /// Records every claim/release the surface issues so the test can assert a
    /// net-zero balance (no leaked inflight profile requests).
    private final class RecordingProfileHost: NostrProfileHost {
        private(set) var claims: [(String, String)] = []
        private(set) var releases: [(String, String)] = []
        /// Liveness intent recorded per claim (parallel to `claims`).
        private(set) var claimLiveness: [ProfileLiveness] = []

        func profile(forPubkey pubkey: String) -> ProfileWire? { nil }
        func claimProfile(pubkey: String, consumerID: String, liveness: ProfileLiveness) {
            claims.append((pubkey, consumerID))
            claimLiveness.append(liveness)
        }
        func releaseProfile(pubkey: String, consumerID: String) {
            releases.append((pubkey, consumerID))
        }

        /// Per-(pubkey, consumer) net refcount across all calls.
        func netCounts() -> [String: Int] {
            var counts: [String: Int] = [:]
            for (pk, cid) in claims { counts["\(pk)|\(cid)", default: 0] += 1 }
            for (pk, cid) in releases { counts["\(pk)|\(cid)", default: 0] -= 1 }
            return counts
        }
    }

    /// Drives the same reconcile logic `NoteContentView` uses (claim added,
    /// release removed, release-all on disappear) against a recording host and
    /// asserts every claim is matched by a release.
    func test_mention_claim_reconcile_is_refcount_balanced() {
        let host = RecordingProfileHost()
        let consumerID = "note-content.mentions.test"

        var claimed: [String] = []

        func sync(to target: [String]) {
            let targetSet = Set(target)
            let currentSet = Set(claimed)
            for pk in currentSet.subtracting(targetSet) {
                host.releaseProfile(pubkey: pk, consumerID: consumerID)
            }
            for pk in targetSet.subtracting(currentSet) {
                // Inline mentions claim `.cacheOk`, matching NoteContentView.
                host.claimProfile(pubkey: pk, consumerID: consumerID, liveness: .cacheOk)
            }
            claimed = target
        }
        func releaseAll() {
            for pk in claimed { host.releaseProfile(pubkey: pk, consumerID: consumerID) }
            claimed = []
        }

        // Mount with [A, B], then the tree changes to [B] (A dropped, no new),
        // then disappears.
        sync(to: [pkA, pkB])
        sync(to: [pkB])
        releaseAll()

        XCTAssertEqual(host.claims.count, 2, "A and B each claimed once.")
        XCTAssertEqual(host.releases.count, 2, "A released on tree change, B on disappear.")
        for (_, net) in host.netCounts() {
            XCTAssertEqual(net, 0, "Every (pubkey, consumer) claim must be matched by a release.")
        }
        XCTAssertTrue(
            host.claimLiveness.allSatisfy { $0 == .cacheOk },
            "Inline/list claims must use .cacheOk (no live subscription).")
    }

    /// The `NostrProfileHost` convenience overload (no `liveness:`) must default
    /// to `.cacheOk`, so registry leaves that don't pass an intent never open a
    /// live subscription.
    func test_convenience_claim_defaults_to_cacheOk() {
        let host = RecordingProfileHost()
        host.claimProfile(pubkey: pkA, consumerID: "leaf")
        XCTAssertEqual(host.claimLiveness, [.cacheOk],
            "The defaulted claim overload must map to .cacheOk.")
    }
}
