import XCTest
@testable import Chirp

/// C3 Performance Fix — Unit coverage for row-level Equatable diffing to
/// prevent re-renders when no visible field changed.
///
/// ## The defect
///
/// The home feed applies a binary FlatBuffers snapshot from the Rust kernel at
/// ≤4Hz. On a quiet feed, the snapshot may have updated KernelMetrics
/// (bytesRx, timing) with no new events and no visible field changes.
/// `TimelineListView.==` (HomeFeedView.swift) delegates item comparison entirely
/// to `TimelineItem.rendersIdentically(to:)`, so correctness of the guard
/// reduces to correctness of that pure function.
///
/// ## The fix
///
/// `rendersIdentically(to:)` compares all visible rendered fields including
/// `relayCount` (rendered by `NoteRowView.relayChip` as `Text("\(item.relayCount)")`).
/// Previously `relayCount` was excluded, causing the relay chip to show a stale
/// count when relay count incremented on idle ticks.
///
/// ## Test scope
///
/// `TimelineListView` is declared `private` in HomeFeedView.swift and cannot
/// be constructed directly from the test target.  Since `TimelineListView.==`
/// is a thin zip-allSatisfy wrapper over `rendersIdentically`, testing the
/// pure function is the complete verification of the guard's correctness.
/// A decision note is logged in docs/perf/pending-user-decisions.md.
@MainActor
final class IdleReRenderTests: XCTestCase {

    // MARK: - Helpers

    /// Returns a baseline `TimelineItem` with all fields populated.
    private func makeItem(
        id: String = "event1",
        authorPubkey: String = "abc123def456abc123def456abc123def456abc123def456abc123def456ab12",
        authorDisplayName: String? = "Alice",
        authorPictureUrl: String? = "https://example.com/alice.jpg",
        authorLnurl: String? = "lnurl1234567890",
        content: String = "Hello, world!",
        contentPreview: String = "Hello, world!",
        createdAt: UInt64 = 1_234_567_890,
        isRepost: Bool = false,
        kind: UInt32 = 1,
        navTargetId: String = "event1",
        relayCount: UInt32 = 1,
        repostInnerContent: String = ""
    ) -> TimelineItem {
        TimelineItem(
            authorDisplayName: authorDisplayName,
            authorLnurl: authorLnurl,
            authorPictureUrl: authorPictureUrl,
            authorPubkey: authorPubkey,
            content: content,
            contentPreview: contentPreview,
            createdAt: createdAt,
            id: id,
            isRepost: isRepost,
            kind: kind,
            navTargetId: navTargetId,
            relayCount: relayCount,
            repostInnerContent: repostInnerContent
        )
    }

    // MARK: - TEST 1: rendersIdentically positive controls

    /// Two items with all visible fields identical must render identically.
    func test_rendersIdentically_trueWhenAllFieldsMatch() {
        let a = makeItem()
        let b = makeItem()
        XCTAssertTrue(a.rendersIdentically(to: b),
            "Items with identical visible fields should render identically")
    }

    // MARK: - TEST 1 negative controls: each visible field drives a re-render

    /// `relayCount` is rendered by `NoteRowView.relayChip` — a count change
    /// must cause a re-render.
    func test_rendersIdentically_falseWhenRelayCountDiffers() {
        let a = makeItem(relayCount: 1)
        let b = makeItem(relayCount: 3)
        XCTAssertFalse(a.rendersIdentically(to: b),
            "Items differing in relayCount should NOT render identically — relayChip displays the count")
    }

    /// `content` is the primary note body — a change must cause a re-render.
    func test_rendersIdentically_falseWhenContentDiffers() {
        let a = makeItem(content: "Hello, world!", contentPreview: "Hello, world!")
        let b = makeItem(content: "Goodbye, world!", contentPreview: "Goodbye, world!")
        XCTAssertFalse(a.rendersIdentically(to: b),
            "Items differing in content should NOT render identically")
    }

    /// `authorDisplayName` is shown in the row header — a change must cause a re-render.
    func test_rendersIdentically_falseWhenAuthorDisplayNameDiffers() {
        let a = makeItem(authorDisplayName: "Alice")
        let b = makeItem(authorDisplayName: "Bob")
        XCTAssertFalse(a.rendersIdentically(to: b),
            "Items differing in authorDisplayName should NOT render identically")
    }

    /// `authorPictureUrl` drives the avatar — a change must cause a re-render.
    func test_rendersIdentically_falseWhenAuthorPictureUrlDiffers() {
        let a = makeItem(authorPictureUrl: "https://example.com/alice.jpg")
        let b = makeItem(authorPictureUrl: "https://example.com/alice-v2.jpg")
        XCTAssertFalse(a.rendersIdentically(to: b),
            "Items differing in authorPictureUrl should NOT render identically")
    }

    /// `createdAt` drives the relative timestamp display — a change must cause a re-render.
    func test_rendersIdentically_falseWhenCreatedAtDiffers() {
        let a = makeItem(createdAt: 1_000_000)
        let b = makeItem(createdAt: 2_000_000)
        XCTAssertFalse(a.rendersIdentically(to: b),
            "Items differing in createdAt should NOT render identically")
    }
}
