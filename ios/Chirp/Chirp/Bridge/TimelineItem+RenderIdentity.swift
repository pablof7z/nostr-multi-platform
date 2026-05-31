import Foundation

// ─────────────────────────────────────────────────────────────────────────
// C3 Performance Fix — Row-level Equatable diffing to prevent re-renders
// on relay-count-only ticks.
//
// The home feed applies a binary FlatBuffers snapshot from the Rust kernel
// at ≤4Hz. On a quiet feed, the snapshot contains no new events but may have
// updated TimelineItem.relayCount (when the same event arrives from another
// relay) and KernelMetrics (bytesRx, timing). The existing TimelineListView
// Equatable guard compared full `items` arrays, so a relayCount-only change
// (1→>1 with no visible difference in the row) caused a full list re-evaluation
// and triggered row body re-renders even though the rendered fields were
// unchanged.
//
// This extension provides a render-relevant equality check that excludes
// `relayCount` and other non-visible fields, allowing TimelineListView.== to
// short-circuit on relay-count-only ticks.
// ─────────────────────────────────────────────────────────────────────────

extension TimelineItem {
    /// Fields the home-feed row actually renders. Excludes `relayCount`,
    /// which transitions 1→>1 on duplicate-relay delivery (kernel/mod.rs:525)
    /// with no visible change — comparing it caused C3 idle re-renders.
    ///
    /// Two TimelineItems are render-identical IFF their visible fields match:
    /// the event identity (id, authorPubkey), display fields (authorDisplayName,
    /// authorPictureUrl, authorLnurl), content (content, contentPreview), and
    /// structural fields (createdAt, isRepost, kind, navTargetId, repostInnerContent).
    /// RelationCount transitions (loading→known) are render-significant and
    /// comparisons use the stored field, not relayCount.
    func rendersIdentically(to other: TimelineItem) -> Bool {
        id == other.id
            && authorPubkey == other.authorPubkey
            && authorDisplayName == other.authorDisplayName
            && authorPictureUrl == other.authorPictureUrl
            && authorLnurl == other.authorLnurl
            && content == other.content
            && contentPreview == other.contentPreview
            && createdAt == other.createdAt
            && isRepost == other.isRepost
            && kind == other.kind
            && navTargetId == other.navTargetId
            && repostInnerContent == other.repostInnerContent
    }
}
