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
// This extension provides a render-relevant equality check that includes all
// visible fields — including `relayCount` which is displayed in the relay chip —
// allowing TimelineListView.== to short-circuit only when no visible field changed.
// ─────────────────────────────────────────────────────────────────────────

extension TimelineItem {
    /// Fields the home-feed row actually renders. Includes `relayCount` because
    /// NoteRowView.relayChip renders `Text("\(item.relayCount)")` whenever
    /// `item.relayCount > 0` — a count change is a visible rendered difference.
    ///
    /// Two TimelineItems are render-identical IFF their visible fields match:
    /// the event identity (id, authorPubkey), display fields (authorDisplayName,
    /// authorPictureUrl, authorLnurl), content (content, contentPreview),
    /// structural fields (createdAt, isRepost, kind, navTargetId, repostInnerContent),
    /// and relayCount (rendered in the relay chip).
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
            && relayCount == other.relayCount
    }
}
