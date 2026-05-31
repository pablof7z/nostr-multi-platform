import XCTest
@testable import Chirp

/// C3 Performance Fix — Unit coverage for row-level Equatable diffing to
/// prevent re-renders on relay-count-only ticks.
///
/// ## The defect
///
/// The home feed applies a binary FlatBuffers snapshot from the Rust kernel at
/// ≤4Hz. On a quiet feed, the snapshot contains no new events but may have
/// updated TimelineItem.relayCount (1→>1 when the same event arrives from
/// another relay) and KernelMetrics (bytesRx, timing). The Equatable guard on
/// `TimelineListView` compared full `items` arrays, so a relayCount-only change
/// with no visible row difference caused the row body to re-evaluate and
/// re-render unnecessarily (~4/sec even on idle feeds).
///
/// ## The fix — two layers
///
/// LAYER 1 — make the existing EquatableView short-circuit on relay-count-only
/// ticks. TimelineItem.relayCount churns on duplicate-relay delivery with no
/// visible change in the rendered row. A new `rendersIdentically(to:)` method
/// compares only the visible fields (id, authorPubkey, content, timestamps, etc.)
/// and excludes relayCount. TimelineListView.== uses this to skip re-renders
/// when only relayCount changed.
///
/// LAYER 2 — eliminate the coarse objectWillChange storm (Phase B, separate PR).
/// Migrate KernelModel from ObservableObject to @Observable and split the single
/// `snapshot` slot into per-concern stored slots with if-changed guards. This
/// prevents metrics-only ticks from invalidating every view that reads @Published,
/// making idle invalidations truly 0 up the tree.
///
/// These tests lock the Layer 1 behavior — the pure-value row diffing that is
/// the core of the C3 fix.
@MainActor
final class IdleReRenderTests: XCTestCase {

    // MARK: - TEST 1: Row-level render identity excludes relayCount

    /// TimelineItem.rendersIdentically(to:) must return true when comparing
    /// two items that differ only in `relayCount`.
    func test_rendersIdentically_ignoresRelayCountChurn() {
        // Construct two timeline items with identical visible fields but
        // different relayCount values (the common quiet-feed case: same event
        // delivered from a second relay).
        let itemV1 = TimelineItem(
            authorDisplayName: "Alice",
            authorLnurl: "lnurl1234567890",
            authorPictureUrl: "https://example.com/alice.jpg",
            authorPubkey: "abc123def456abc123def456abc123def456abc123def456abc123def456ab",
            content: "Hello, world!",
            contentPreview: "Hello, world!",
            createdAt: 1234567890,
            id: "event1",
            isRepost: false,
            kind: 1,
            navTargetId: "event1",
            relayCount: 1,
            repostInnerContent: ""
        )

        let itemV2 = TimelineItem(
            authorDisplayName: "Alice",
            authorLnurl: "lnurl1234567890",
            authorPictureUrl: "https://example.com/alice.jpg",
            authorPubkey: "abc123def456abc123def456abc123def456abc123def456abc123def456ab",
            content: "Hello, world!",
            contentPreview: "Hello, world!",
            createdAt: 1234567890,
            id: "event1",
            isRepost: false,
            kind: 1,
            navTargetId: "event1",
            relayCount: 3, // Changed — duplicate relay delivery
            repostInnerContent: ""
        )

        // Verify that rendersIdentically returns true despite relayCount difference.
        XCTAssertTrue(
            itemV1.rendersIdentically(to: itemV2),
            "Items differing only in relayCount should render identically"
        )
    }

    /// Negative control: rendersIdentically must return false when a visible
    /// field differs (e.g., content), even if relayCount is the same.
    func test_rendersIdentically_detecdsContentChanges() {
        let itemV1 = TimelineItem(
            authorDisplayName: "Alice",
            authorLnurl: "lnurl1234567890",
            authorPictureUrl: "https://example.com/alice.jpg",
            authorPubkey: "abc123def456abc123def456abc123def456abc123def456abc123def456ab",
            content: "Hello, world!",
            contentPreview: "Hello, world!",
            createdAt: 1234567890,
            id: "event1",
            isRepost: false,
            kind: 1,
            navTargetId: "event1",
            relayCount: 1,
            repostInnerContent: ""
        )

        let itemV2 = TimelineItem(
            authorDisplayName: "Alice",
            authorLnurl: "lnurl1234567890",
            authorPictureUrl: "https://example.com/alice.jpg",
            authorPubkey: "abc123def456abc123def456abc123def456abc123def456abc123def456ab",
            content: "Goodbye, world!", // Changed — visible field
            contentPreview: "Goodbye, world!",
            createdAt: 1234567890,
            id: "event1",
            isRepost: false,
            kind: 1,
            navTargetId: "event1",
            relayCount: 1, // Unchanged
            repostInnerContent: ""
        )

        // Verify that rendersIdentically returns false when content differs.
        XCTAssertFalse(
            itemV1.rendersIdentically(to: itemV2),
            "Items differing in content should NOT render identically"
        )
    }

    // MARK: - TEST 2: TimelineListView.== uses render identity for items comparison

    /// TimelineListView.== must short-circuit when items differ only in
    /// relayCount, returning true so the row body does not re-evaluate.
    func test_timelineListViewEquatable_ignoresRelayCountOnly() {
        let item1 = TimelineItem(
            authorDisplayName: "Bob",
            authorLnurl: "lnurl9999999999",
            authorPictureUrl: "https://example.com/bob.jpg",
            authorPubkey: "feed123def456feed123def456feed123def456feed123def456feed123def456",
            content: "Testing row diffing",
            contentPreview: "Testing row diffing",
            createdAt: 9999999999,
            id: "testEvent1",
            isRepost: false,
            kind: 1,
            navTargetId: "testEvent1",
            relayCount: 1,
            repostInnerContent: ""
        )

        let item2 = TimelineItem(
            authorDisplayName: "Bob",
            authorLnurl: "lnurl9999999999",
            authorPictureUrl: "https://example.com/bob.jpg",
            authorPubkey: "feed123def456feed123def456feed123def456feed123def456feed123def456",
            content: "Testing row diffing",
            contentPreview: "Testing row diffing",
            createdAt: 9999999999,
            id: "testEvent1",
            isRepost: false,
            kind: 1,
            navTargetId: "testEvent1",
            relayCount: 5, // Changed only
            repostInnerContent: ""
        )

        let root = ChirpRootCard(
            card: ChirpEventCard(
                id: "testEvent1",
                authorDisplayName: "Bob",
                authorPictureUrl: "https://example.com/bob.jpg",
                authorPubkey: "feed123def456feed123def456feed123def456feed123def456feed123def456",
                content: "Testing row diffing",
                contentPreview: "Testing row diffing",
                createdAt: 9999999999,
                isRepost: false,
                kind: 1,
                navTargetId: "testEvent1",
                authorLnurl: "lnurl9999999999",
                repostInnerContent: "",
                relationCounts: NoteRelationCounts(
                    likeCount: 0,
                    replyCount: 0,
                    repostCount: 0,
                    zapCount: 0
                )
            ),
            attribution: []
        )

        // Construct two identical TimelineListView instances, differing only in
        // the items array (which itself differs only in relayCount).
        let view1 = TimelineListView(
            roots: [root],
            nextCursor: nil,
            items: [item1],
            mentionProfiles: [:],
            onRefresh: {},
            onLike: { _ in },
            onRepost: { _, _ in },
            onZap: { _, _, _ in },
            onLoadMore: { _ in }
        )

        let view2 = TimelineListView(
            roots: [root],
            nextCursor: nil,
            items: [item2], // Only relayCount differs
            mentionProfiles: [:],
            onRefresh: {},
            onLike: { _ in },
            onRepost: { _, _ in },
            onZap: { _, _, _ in },
            onLoadMore: { _ in }
        )

        // Verify that the two views compare equal despite the relayCount churn.
        XCTAssertEqual(
            view1,
            view2,
            "TimelineListView instances should be equal when items differ only in relayCount"
        )
    }

    /// Negative control: TimelineListView.== must return false when roots,
    /// nextCursor, or mentionProfiles differ, or when items differ in a visible
    /// field (not just relayCount).
    func test_timelineListViewEquatable_detectsVisibleChanges() {
        let root1 = ChirpRootCard(
            card: ChirpEventCard(
                id: "event1",
                authorDisplayName: "Charlie",
                authorPictureUrl: "https://example.com/charlie.jpg",
                authorPubkey: "charlie123def456charlie123def456charlie123def456charlie123def456abc",
                content: "First event",
                contentPreview: "First event",
                createdAt: 5555555555,
                isRepost: false,
                kind: 1,
                navTargetId: "event1",
                authorLnurl: "lnurl1111111111",
                repostInnerContent: "",
                relationCounts: NoteRelationCounts(likeCount: 0, replyCount: 0, repostCount: 0, zapCount: 0)
            ),
            attribution: []
        )

        let root2 = ChirpRootCard(
            card: ChirpEventCard(
                id: "event2", // Different card
                authorDisplayName: "Diana",
                authorPictureUrl: "https://example.com/diana.jpg",
                authorPubkey: "diana123def456diana123def456diana123def456diana123def456abc12345",
                content: "Second event",
                contentPreview: "Second event",
                createdAt: 6666666666,
                isRepost: false,
                kind: 1,
                navTargetId: "event2",
                authorLnurl: "lnurl2222222222",
                repostInnerContent: "",
                relationCounts: NoteRelationCounts(likeCount: 5, replyCount: 0, repostCount: 0, zapCount: 0)
            ),
            attribution: []
        )

        let view1 = TimelineListView(
            roots: [root1],
            nextCursor: nil,
            items: [],
            mentionProfiles: [:],
            onRefresh: {},
            onLike: { _ in },
            onRepost: { _, _ in },
            onZap: { _, _, _ in },
            onLoadMore: { _ in }
        )

        let view2 = TimelineListView(
            roots: [root2], // Different roots
            nextCursor: nil,
            items: [],
            mentionProfiles: [:],
            onRefresh: {},
            onLike: { _ in },
            onRepost: { _, _ in },
            onZap: { _, _, _ in },
            onLoadMore: { _ in }
        )

        // Verify that the two views do NOT compare equal.
        XCTAssertNotEqual(
            view1,
            view2,
            "TimelineListView instances should NOT be equal when roots differ"
        )
    }
}
