import Foundation
import Observation

/// Home feed — the merge of friend highlights and friend-surfaced reads into
/// a single chronological stream. Composes `HighlightsStore` and `ReadsStore`
/// (owning both for the lifetime of the view) and recomputes a deduped,
/// sorted `items` array whenever either side changes.
///
/// Highlights are grouped by source (article address or sourceUrl) regardless
/// of who highlighted them — multiple people highlighting the same article
/// land in one module. The single-highlight case uses the same shape (just
/// a one-element array). Dedup rule: if any friend highlighted an article,
/// that article is dropped from the reads side.
@MainActor
@Observable
final class HomeFeedStore {
    enum Item: Hashable {
        /// One or more highlights on the same source (article / web URL).
        /// Always non-empty. The view component renders the same module
        /// shape for count == 1 and count > 1.
        case highlights([HydratedHighlight])
        case read(ReadingFeedItem)

        var sortKey: UInt64 {
            switch self {
            case .highlights(let hs): return hs.compactMap(\.highlight.createdAt).max() ?? 0
            case .read(let r):        return r.latestActivityAt
            }
        }

        var stableId: String {
            switch self {
            case .highlights(let hs):
                let h = hs[0]
                let addr = h.highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
                let src  = addr.isEmpty
                    ? h.highlight.sourceUrl.trimmingCharacters(in: .whitespacesAndNewlines)
                    : addr
                if !src.isEmpty { return "h:src:\(src)" }
                // Sourceless lone highlight — fall back to event id.
                return "h:evt:\(h.highlight.eventId)"
            case .read(let r):
                return "r:30023:\(r.article.pubkey):\(r.article.identifier)"
            }
        }
    }

    var items: [Item] = []
    var isLoadingInitial: Bool = true

    @ObservationIgnored let highlights: HighlightsStore
    @ObservationIgnored let reads: ReadsStore

    @ObservationIgnored private var observing: Bool = false

    init(safeCore: SafeHighlighterCore, eventBridge: EventBridge?) {
        self.highlights = HighlightsStore(safeCore: safeCore, eventBridge: eventBridge)
        self.reads = ReadsStore(safeCore: safeCore, eventBridge: eventBridge)
    }

    func start() async {
        async let h: Void = highlights.start()
        async let r: Void = reads.start()
        _ = await (h, r)
        recompute()
        isLoadingInitial = false
        observing = true
        observeHighlights()
        observeReads()
    }

    func stop() {
        observing = false
        highlights.stop()
        reads.stop()
    }

    private func observeHighlights() {
        withObservationTracking {
            _ = highlights.items
        } onChange: { [weak self] in
            Task { @MainActor in
                guard let self, self.observing else { return }
                self.recompute()
                self.observeHighlights()
            }
        }
    }

    private func observeReads() {
        withObservationTracking {
            _ = reads.items
        } onChange: { [weak self] in
            Task { @MainActor in
                guard let self, self.observing else { return }
                self.recompute()
                self.observeReads()
            }
        }
    }

    /// Source-only grouping key. Uses the canonical `sourceReferenceKey`
    /// produced by core, which covers articles (`a:`), nostr events (`e:`),
    /// external entities like ISBN-keyed books (`i:`), and plain URLs (`r:`).
    /// Returns nil for highlights with no identifiable source (those land as
    /// solo entries via event id).
    private func groupKey(for h: HydratedHighlight) -> String? {
        let key = h.highlight.sourceReferenceKey.trimmingCharacters(in: .whitespacesAndNewlines)
        return key.isEmpty ? nil : key
    }

    private func recompute() {
        // Bucket highlights by source, preserving first-seen order.
        var groupMap: [String: [HydratedHighlight]] = [:]
        var groupOrder: [String] = []

        for h in highlights.items {
            let key = groupKey(for: h) ?? "solo:\(h.highlight.eventId)"
            if groupMap[key] == nil {
                groupOrder.append(key)
                groupMap[key] = []
            }
            groupMap[key]!.append(h)
        }

        let highlightedAddresses: Set<String> = Set(
            highlights.items.compactMap { h in
                let addr = h.highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
                return addr.isEmpty ? nil : addr
            }
        )

        var merged: [Item] = []
        merged.reserveCapacity(groupOrder.count + reads.items.count)

        for key in groupOrder {
            let group = groupMap[key]!
            // Sort within group chronologically (oldest first = reading order).
            let sorted = group.sorted { ($0.highlight.createdAt ?? 0) < ($1.highlight.createdAt ?? 0) }
            merged.append(.highlights(sorted))
        }

        for r in reads.items {
            let addr = "30023:\(r.article.pubkey):\(r.article.identifier)"
            if highlightedAddresses.contains(addr) { continue }
            merged.append(.read(r))
        }

        merged.sort { $0.sortKey > $1.sortKey }
        items = merged
    }
}
