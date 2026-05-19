import SwiftUI

/// The community home's Home tab — a stream of artifact-grouped highlight
/// modules, identical in shape to the Highlights tab. Each lane pairs one
/// artifact with the room's recent highlights on it; dormant lanes (no
/// highlights and no comments) are filtered out — the Library tab is the
/// place to browse every artifact regardless of activity.
///
/// Highlight data flows in two streams because the Rust core's
/// `get_highlights(groupId:)` filters on `#h` tags that kind:9802 events
/// don't carry (community association lives on the kind:16 repost, not
/// on the highlight itself). So for articles we fetch per-address via
/// `get_highlights_for_article`. Books and podcasts don't yet have an
/// equivalent per-artifact query; their lanes appear without pull-quotes
/// until that lands.
struct RoomLanesView: View {
    let artifacts: [ArtifactRecord]
    let highlights: [HydratedHighlight]
    let highlightsByReference: [String: [HighlightRecord]]
    let commentsByReference: [String: [CommentRecord]]
    let isLoading: Bool
    let onShareToCommunity: (ArtifactRecord) -> Void

    var body: some View {
        if isLoading && artifacts.isEmpty {
            ProgressView()
                .controlSize(.large)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if visibleLanes.isEmpty {
            ContentUnavailableView(
                "Nothing here yet",
                systemImage: "square.stack.3d.up",
                description: Text("Highlights from the room's library will appear here.")
            )
        } else {
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(Array(visibleLanes.enumerated()), id: \.element.id) { index, lane in
                        laneView(for: lane)
                        if index < visibleLanes.count - 1 {
                            Rectangle()
                                .fill(Color.highlighterRule)
                                .frame(height: 1)
                        }
                    }
                }
                .padding(.horizontal, 12)
            }
            .background(Color.highlighterPaper.ignoresSafeArea())
        }
    }

    private var visibleLanes: [Lane] {
        Lane.build(
            artifacts: artifacts,
            highlights: highlights,
            highlightsByReference: highlightsByReference,
            commentsByReference: commentsByReference
        )
        .filter { !$0.isDormant }
    }

    @ViewBuilder
    private func laneView(for lane: Lane) -> some View {
        if !lane.highlights.isEmpty {
            NavigationLink(value: lane.artifact) {
                HighlightFeedCardView(items: lane.highlights)
            }
            .buttonStyle(.plain)
            .contextMenu {
                Button {
                    onShareToCommunity(lane.artifact)
                } label: {
                    Label("Share to community", systemImage: "square.and.arrow.up")
                }
            }
        }
    }
}

// MARK: - Lane model

/// A single lane on the community home: an artifact together with the
/// community's recent highlights and NIP-22 comments on it.
struct Lane: Identifiable {
    let id: String
    let artifact: ArtifactRecord
    /// Newest-first.
    let highlights: [HydratedHighlight]
    /// Newest-first.
    let comments: [CommentRecord]

    var latestActivity: UInt64? {
        var ts: UInt64 = 0
        if let h = highlights.compactMap({ $0.highlight.createdAt }).max() { ts = max(ts, h) }
        if let c = comments.compactMap({ $0.createdAt }).max() { ts = max(ts, c) }
        if ts > 0 { return ts }
        return artifact.createdAt
    }

    var isDormant: Bool { highlights.isEmpty && comments.isEmpty }

    /// Build lanes from `artifacts` + reference-scoped highlight / comment
    /// fetches. `highlightsByReference` is keyed `"<lowercase>:<value>"`,
    /// `commentsByReference` is keyed `"<UPPERCASE>:<value>"` (NIP-22
    /// root scope convention). Falls back to a permissive match against
    /// the group-scoped `highlights` stream for any artifact that didn't
    /// pull a per-reference result.
    static func build(
        artifacts: [ArtifactRecord],
        highlights: [HydratedHighlight],
        highlightsByReference: [String: [HighlightRecord]],
        commentsByReference: [String: [CommentRecord]]
    ) -> [Lane] {
        var lanes: [Lane] = artifacts.map { art in
            var highlightBucket: [HydratedHighlight] = []
            var commentBucket: [CommentRecord] = []

            let (lowerTag, upperTag, value) = referenceTriple(for: art)
            if !value.isEmpty {
                if !lowerTag.isEmpty, let recs = highlightsByReference["\(lowerTag):\(value)"] {
                    highlightBucket.append(contentsOf: recs.map { rec in
                        HydratedHighlight(
                            highlight: rec,
                            artifact: art,
                            sharedByEventId: nil,
                            sharedByPubkey: nil
                        )
                    })
                }
                if !upperTag.isEmpty, let recs = commentsByReference["\(upperTag):\(value)"] {
                    commentBucket = recs
                }
            }

            for h in highlights where matches(h, art) {
                if highlightBucket.contains(where: { $0.highlight.eventId == h.highlight.eventId }) {
                    continue
                }
                highlightBucket.append(h)
            }

            highlightBucket.sort { ($0.highlight.createdAt ?? 0) > ($1.highlight.createdAt ?? 0) }
            commentBucket.sort { ($0.createdAt ?? 0) > ($1.createdAt ?? 0) }

            return Lane(
                id: art.shareEventId.isEmpty ? art.preview.id : art.shareEventId,
                artifact: art,
                highlights: highlightBucket,
                comments: commentBucket
            )
        }

        lanes.sort { a, b in
            switch (a.isDormant, b.isDormant) {
            case (false, true): return true
            case (true, false): return false
            default: return (a.latestActivity ?? 0) > (b.latestActivity ?? 0)
            }
        }
        return lanes
    }

    /// Permissive predicate for the group-scoped `highlights` fallback —
    /// used only when the per-reference fetch hasn't provided a match.
    private static func matches(_ h: HydratedHighlight, _ art: ArtifactRecord) -> Bool {
        let hl = h.highlight
        let pv = art.preview

        if !pv.referenceTagName.isEmpty, !pv.referenceTagValue.isEmpty {
            let artKey = "\(pv.referenceTagName):\(pv.referenceTagValue)"
            if !hl.sourceReferenceKey.isEmpty, hl.sourceReferenceKey == artKey {
                return true
            }
        }

        if !hl.artifactAddress.isEmpty {
            if hl.artifactAddress == pv.referenceTagValue { return true }
            if hl.artifactAddress == pv.highlightTagValue { return true }
        }

        if !hl.externalReference.isEmpty {
            if hl.externalReference == pv.referenceTagValue { return true }
            if hl.externalReference == pv.highlightTagValue { return true }
            if !pv.podcastItemGuid.isEmpty,
               hl.externalReference == "podcast:item:guid:\(pv.podcastItemGuid)" {
                return true
            }
        }

        if !hl.eventReference.isEmpty {
            if hl.eventReference == pv.referenceTagValue { return true }
            if hl.eventReference == art.shareEventId { return true }
        }

        if !hl.sourceUrl.isEmpty {
            if hl.sourceUrl == pv.url { return true }
            if !pv.audioUrl.isEmpty, hl.sourceUrl == pv.audioUrl { return true }
        }

        return false
    }

    /// Returns `(lowercaseTag, uppercaseTag, value)` for the artifact's
    /// primary reference, or empty strings for artifacts without one.
    private static func referenceTriple(for art: ArtifactRecord) -> (String, String, String) {
        let pv = art.preview
        if !pv.referenceTagName.isEmpty, !pv.referenceTagValue.isEmpty {
            return (pv.referenceTagName.lowercased(), pv.referenceTagName.uppercased(), pv.referenceTagValue)
        }
        if !pv.highlightTagName.isEmpty, !pv.highlightTagValue.isEmpty {
            return (pv.highlightTagName.lowercased(), pv.highlightTagName.uppercased(), pv.highlightTagValue)
        }
        return ("", "", "")
    }
}
