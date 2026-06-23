import Foundation

// ── Feed open/close operations ────────────────────────────────────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).

extension KernelHandle {
    func openAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_chirp_open_author_feed(raw, $0) }
    }

    func openThread(eventID: String) {
        eventID.withCString { nmp_app_chirp_open_thread_feed(raw, $0) }
    }

    // M2 (ADR-0042): `openFirehose(tag:)` and the `nmp_app_open_firehose_tag`
    // C symbol it wrapped were deleted. A hashtag feed is now expressed through
    // the Chirp-owned tag-feed seam, which declares primary kind `[1]`, derives
    // NIP-18 repost wrapper acquisition, and opens the compiled `#t` filter at
    // `.global` scope (D0-correct).

    /// M2 (ADR-0042) — generic feed-subscription open. `filterJSON` is a
    /// verbatim NIP-01 REQ filter.
    /// Declared feeds should pass primary kinds only through their typed seam;
    /// protocol adapters derive repost wrappers. `consumerID` refcounts owners so
    /// repeated opens of the same filter share one live subscription; `scope`
    /// is `.activeAccount` (re-route on switch) or `.global` (account-agnostic).
    /// Generic replacement for the deleted `openFirehose`. V-112 (ADR-0042):
    /// `openAuthor` / `openThread` now delegate to the chirp feed seam below.
    func openInterest(filterJSON: String, consumerID: String, scope: InterestScope) {
        filterJSON.withCString { filterPtr in
            consumerID.withCString { consumerPtr in
                nmp_app_open_interest(raw, filterPtr, consumerPtr, scope.rawValue)
            }
        }
    }

    /// M2 (ADR-0042) — detach one owner from a feed interest opened with
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
    /// Tears down the author-subscription so the kernel's wire_subs count
    /// returns to baseline. Call from `.onDisappear` on the AuthorView
    /// (ProfileView) to prevent sub-leaks on navigation pop.
    func closeAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_chirp_close_author_feed(raw, $0) }
    }

    /// Signal that the thread for `eventID` is no longer visible.
    /// Symmetric counterpart to `openThread`; call from `.onDisappear`
    /// on the ThreadScreen to release the thread subscription.
    func closeThread(eventID: String) {
        eventID.withCString { nmp_app_chirp_close_thread_feed(raw, $0) }
    }

    func openTimeline() {
        nmp_app_chirp_open_home_feed(raw)
    }

    func closeTimeline() {
        nmp_app_chirp_close_home_feed(raw)
    }
}
