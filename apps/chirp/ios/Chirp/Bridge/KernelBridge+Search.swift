import Foundation

/// NIP-50 higher-order search (nmp-nip50) C-ABI passthroughs.
///
/// THIN SHELL: every method here is a verbatim `withCString` forward to a
/// `nmp_app_search_*` symbol: zero search logic. The kernel owns query
/// validation, relay selection, cache-FTS scanning, dedup, and result ordering.
///
/// Kept in this extension file so the search feature is self-contained and the
/// host bridge files stay within their size budget.
extension KernelHandle {
    /// Open a NIP-50 search session over the `nmp_app_search_open` C ABI.
    ///
    /// `requestJSON` is the serde wire of a `nmp_nip50::SearchRequest`, for
    /// example `{"query":"jack","scope":"Users","targets":"UserPreferred","max_hits":50}`.
    /// The kernel re-runs bounded-query validation and registers one typed
    /// `N50S` result sidecar under `nmp.nip50.search.<sessionID>`.
    func openSearch(requestJSON: String, sessionID: String) {
        requestJSON.withCString { requestPtr in
            sessionID.withCString { sessionPtr in
                nmp_app_search_open(raw, requestPtr, sessionPtr)
            }
        }
    }

    /// Close a search session opened with `openSearch`. Idempotent.
    func closeSearch(sessionID: String) {
        sessionID.withCString { nmp_app_search_close(raw, $0) }
    }

    /// Pull the current typed `N50S` search-results buffer for `sessionID` via
    /// the two-call size probe exposed by `nmp_app_search_snapshot`.
    func searchSnapshotBytes(sessionID: String) -> Data? {
        sessionID.withCString { sessionPtr -> Data? in
            let needed = nmp_app_search_snapshot(raw, sessionPtr, nil, 0)
            guard needed > 0 else { return nil }
            let count = Int(needed)
            var buffer = [UInt8](repeating: 0, count: count)
            let written = buffer.withUnsafeMutableBufferPointer { ptr in
                nmp_app_search_snapshot(raw, sessionPtr, ptr.baseAddress, UInt(count))
            }
            guard Int(written) == count else { return nil }
            return Data(buffer)
        }
    }
}
