import Foundation
import Observation

/// View-scoped reactive state for a single community's room home.
/// Lifetime is tied to the SwiftUI view that creates it — allocated on
/// `.task { }`, deallocated on view disappear. Owns its subscription
/// handle so granular Observation tracks only this room's data.
///
/// Data comes from nostrdb via the Rust core; this class never fabricates
/// or caches data that isn't also in nostrdb.
@MainActor
@Observable
final class RoomStore {
    private(set) var artifacts: [ArtifactRecord] = []
    private(set) var highlights: [HydratedHighlight] = []
    /// Per-artifact highlights, keyed by `"<tagName>:<tagValue>"` (e.g.
    /// `"a:30023:pk:d"` for articles, `"i:isbn:…"` for books, `"r:<url>"`
    /// for podcasts). Populated by `get_highlights_for_reference` because
    /// the group-scoped `get_highlights(groupId:)` filters on `#h` which
    /// kind:9802 events don't carry.
    private(set) var highlightsByReference: [String: [HighlightRecord]] = [:]
    /// NIP-22 comments (kind:1111) per artifact, keyed by the UPPERCASE
    /// scope (`"A:30023:pk:d"` / `"I:isbn:…"` / `"E:<event-id>"`).
    private(set) var commentsByReference: [String: [CommentRecord]] = [:]
    private(set) var isLoading: Bool = true
    private(set) var loadError: String?

    @ObservationIgnored private var groupId: String?
    @ObservationIgnored private var core: SafeHighlighterCore?
    @ObservationIgnored private weak var bridge: EventBridge?
    @ObservationIgnored private var subscriptionHandle: UInt64?

    /// Called from the View's `.task { }`. Reads nostrdb immediately for
    /// instant offline rendering, then installs a live subscription so
    /// incoming events flow in as deltas routed by `EventBridge`.
    func start(groupId: String, core: SafeHighlighterCore, bridge: EventBridge?) async {
        self.groupId = groupId
        self.core = core
        self.bridge = bridge
        isLoading = true
        loadError = nil

        async let artifactsFetch = core.getArtifacts(groupId: groupId)
        async let highlightsFetch = core.getHighlights(groupId: groupId)

        do {
            artifacts = try await artifactsFetch
            highlights = try await highlightsFetch
        } catch {
            loadError = (error as? CoreError).map { "\($0)" }
        }
        isLoading = false

        await refreshReferenceQueries()

        do {
            let handle = try await core.subscribeRoom(groupId: groupId)
            subscriptionHandle = handle
            bridge?.registerRoom(self, handle: handle)
        } catch {
            // Subscription failure leaves cache-only rendering working.
        }
    }

    func stop() {
        if let handle = subscriptionHandle, let core {
            Task { await core.unsubscribe(handle) }
            bridge?.unregister(handle: handle)
        }
        subscriptionHandle = nil
    }

    // MARK: - Delta application (called by EventBridge)

    func apply(artifact: ArtifactRecord) {
        if let i = artifacts.firstIndex(where: { $0.shareEventId == artifact.shareEventId }) {
            artifacts[i] = artifact
        } else {
            let inserted = artifacts + [artifact]
            artifacts = inserted.sorted { ($0.createdAt ?? 0) > ($1.createdAt ?? 0) }
        }
        Task { await self.refreshReferenceQueries(for: artifact) }
    }

    func apply(highlight: HydratedHighlight) {
        if let i = highlights.firstIndex(where: { $0.highlight.eventId == highlight.highlight.eventId }) {
            highlights[i] = highlight
        } else {
            highlights.append(highlight)
        }
        // Merge into the reference-scoped bucket too so per-artifact lanes
        // reflect live arrivals without waiting for the next refresh.
        if let (tagName, tagValue) = lowercaseReference(for: highlight.highlight) {
            let key = "\(tagName):\(tagValue)"
            var bucket = highlightsByReference[key] ?? []
            if let i = bucket.firstIndex(where: { $0.eventId == highlight.highlight.eventId }) {
                bucket[i] = highlight.highlight
            } else {
                bucket.append(highlight.highlight)
            }
            bucket.sort { ($0.createdAt ?? 0) > ($1.createdAt ?? 0) }
            highlightsByReference[key] = bucket
        }
    }

    // MARK: - Reference queries

    /// Runs `get_highlights_for_reference` + `get_comments_for_reference`
    /// for every artifact in `artifacts`. Each artifact dispatches both
    /// fetches in parallel; failures keep whatever was previously there.
    private func refreshReferenceQueries() async {
        let targets: [ReferenceTarget] = artifacts.compactMap(referenceTarget(for:))
        guard !targets.isEmpty, let core else { return }

        struct FetchResult {
            let target: ReferenceTarget
            let highlights: [HighlightRecord]?
            let comments: [CommentRecord]?
        }

        await withTaskGroup(of: FetchResult.self) { group in
            for target in targets {
                group.addTask {
                    let hl: [HighlightRecord]? = try? await core.getHighlightsForReference(
                        tagName: target.lowercaseTag,
                        tagValue: target.value
                    )
                    let cm: [CommentRecord]? = try? await core.getCommentsForReference(
                        tagName: target.uppercaseTag,
                        tagValue: target.value
                    )
                    return FetchResult(target: target, highlights: hl, comments: cm)
                }
            }
            for await result in group {
                let t = result.target
                if let hl = result.highlights {
                    highlightsByReference["\(t.lowercaseTag):\(t.value)"] = hl
                }
                if let cm = result.comments {
                    commentsByReference["\(t.uppercaseTag):\(t.value)"] = cm
                }
            }
        }
    }

    private func refreshReferenceQueries(for artifact: ArtifactRecord) async {
        guard let core, let target = referenceTarget(for: artifact) else { return }
        let hl: [HighlightRecord]? = try? await core.getHighlightsForReference(
            tagName: target.lowercaseTag,
            tagValue: target.value
        )
        let cm: [CommentRecord]? = try? await core.getCommentsForReference(
            tagName: target.uppercaseTag,
            tagValue: target.value
        )
        if let hl {
            highlightsByReference["\(target.lowercaseTag):\(target.value)"] = hl
        }
        if let cm {
            commentsByReference["\(target.uppercaseTag):\(target.value)"] = cm
        }
    }

    // MARK: - Reference targets

    /// The lowercase + uppercase tag pair to query for an artifact, plus
    /// the shared value. Returns `nil` for artifacts lacking a usable
    /// reference (no `i` / `a` / `r` information).
    private struct ReferenceTarget: Sendable {
        let lowercaseTag: String  // "a" | "i" | "e" | "r"
        let uppercaseTag: String  // "A" | "I" | "E" | "R"
        let value: String
    }

    private func referenceTarget(for artifact: ArtifactRecord) -> ReferenceTarget? {
        let pv = artifact.preview
        if !pv.referenceTagName.isEmpty, !pv.referenceTagValue.isEmpty {
            return ReferenceTarget(
                lowercaseTag: pv.referenceTagName.lowercased(),
                uppercaseTag: pv.referenceTagName.uppercased(),
                value: pv.referenceTagValue
            )
        }
        if !pv.highlightTagName.isEmpty, !pv.highlightTagValue.isEmpty {
            return ReferenceTarget(
                lowercaseTag: pv.highlightTagName.lowercased(),
                uppercaseTag: pv.highlightTagName.uppercased(),
                value: pv.highlightTagValue
            )
        }
        return nil
    }

    private func lowercaseReference(for highlight: HighlightRecord) -> (String, String)? {
        if !highlight.artifactAddress.isEmpty {
            return ("a", highlight.artifactAddress)
        }
        if !highlight.eventReference.isEmpty {
            return ("e", highlight.eventReference)
        }
        if !highlight.sourceUrl.isEmpty {
            return ("r", highlight.sourceUrl)
        }
        return nil
    }
}
