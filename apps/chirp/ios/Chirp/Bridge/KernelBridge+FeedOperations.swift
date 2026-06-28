import Foundation

// ── Feed open/close operations ────────────────────────────────────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).

extension KernelHandle {
    func openAuthor(pubkey: String) {
        openFeed(
            projectionKey: "nmp.feed.author.\(pubkey)",
            render: "Flat",
            acquisition: ["Authors": ["authors": [pubkey]]]
        )
    }

    func openThread(eventID: String) {
        openFeed(
            projectionKey: "nmp.feed.thread.\(eventID)",
            render: "Flat",
            acquisition: ["Referrer": ["event_id": eventID]]
        )
    }

    // M2 (ADR-0042): `openFirehose(tag:)` and the `nmp_app_open_firehose_tag`
    // C symbol it wrapped were deleted. A hashtag feed is now expressed through
    // the Chirp-owned tag-feed seam, which declares primary kind `[1]`, derives
    // NIP-18 repost wrapper acquisition, and opens the compiled `#t` filter at
    // `.global` scope (D0-correct).

    /// M2 (ADR-0042) — low-level static interest open. `filterJSON` is a
    /// verbatim NIP-01 REQ filter after feed/source policy has already been
    /// compiled elsewhere.
    /// App feeds should use `openFeed`; dynamic sources such as active follows
    /// are ReducedSource feed sessions, not native-computed author lists.
    /// `consumerID` refcounts owners so repeated opens of the same filter share
    /// one live subscription; `scope` is `.activeAccount` or `.global`.
    func openInterest(filterJSON: String, consumerID: String, scope: InterestScope) {
        filterJSON.withCString { filterPtr in
            consumerID.withCString { consumerPtr in
                nmp_app_open_interest(raw, filterPtr, consumerPtr, scope.rawValue)
            }
        }
    }

    /// M2 (ADR-0042) — detach one owner from a low-level interest opened with
    /// `openInterest`. The live subscription is dropped on the last owner's
    /// close. Pass the SAME `filterJSON` / `consumerID` / `scope` the open used.
    func closeInterest(filterJSON: String, consumerID: String, scope: InterestScope) {
        filterJSON.withCString { filterPtr in
            consumerID.withCString { consumerPtr in
                nmp_app_close_interest(raw, filterPtr, consumerPtr, scope.rawValue)
            }
        }
    }

    /// Signal that the author feed for `pubkey` is no longer visible.
    func closeAuthor(pubkey: String) {
        closeFeed(projectionKey: "nmp.feed.author.\(pubkey)")
    }

    /// Signal that the thread for `eventID` is no longer visible.
    func closeThread(eventID: String) {
        closeFeed(projectionKey: "nmp.feed.thread.\(eventID)")
    }

    func openTimeline() {
        openFeed(
            projectionKey: "nmp.feed.home",
            render: "OpCentric",
            acquisition: "ActiveUserFollows"
        )
    }

    func closeTimeline() {
        closeFeed(projectionKey: "nmp.feed.home")
    }

    func closeAllOpenFeeds() {
        let handles = Array(feedHandlesByKey.values)
        feedHandlesByKey.removeAll()
        for handle in handles {
            handle.withCString { nmp_app_close_feed(raw, $0) }
        }
    }

    @discardableResult
    private func openFeed(projectionKey: String, render: String, acquisition: Any) -> Bool {
        guard feedHandlesByKey[projectionKey] == nil else { return true }
        let params: [String: Any] = [
            "primary_kinds": [1],
            "render": render,
            "acquisition": acquisition,
            "admission": "All",
            "ranking": "ChronologicalDesc",
            "window": ["initial_limit": 80],
            "projection": projectionKey,
        ]
        guard
            let data = try? JSONSerialization.data(withJSONObject: params),
            let json = String(data: data, encoding: .utf8)
        else {
            return false
        }
        let resultPtr = json.withCString { nmp_app_open_feed(raw, $0) }
        guard let resultPtr else { return false }
        defer { nmp_free_string(resultPtr) }
        let handle = String(cString: resultPtr)
        guard !handle.contains("\"error\"") else { return false }
        feedHandlesByKey[projectionKey] = handle
        return true
    }

    private func closeFeed(projectionKey: String) {
        guard let handle = feedHandlesByKey.removeValue(forKey: projectionKey) else {
            return
        }
        handle.withCString { nmp_app_close_feed(raw, $0) }
    }
}
