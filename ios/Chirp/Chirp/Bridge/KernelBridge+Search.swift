import Foundation

/// NIP-50 higher-order search (nmp-nip50) C-ABI passthroughs.
///
/// THIN SHELL: every method here is a verbatim `withCString` forward to a
/// `nmp_app_search_*` symbol — zero search logic. The kernel owns query
/// validation, relay selection (UserPreferred → the active account's kind:10007
/// list, wired by `register_defaults`), the cache-FTS scan, dedup, and result
/// ordering.
///
/// Kept in this extension file (not inlined into `KernelBridge.swift`) so the
/// search feature is fully self-contained and the host god-files stay at their
/// grandfathered size baseline.
extension KernelHandle {
    /// Open a NIP-50 search session over the `nmp_app_search_open` C-ABI.
    ///
    /// `requestJSON` is the serde wire of a `nmp_nip50::SearchRequest`, e.g.
    /// `{"query":"jack","scope":"Users","targets":"UserPreferred","max_hits":50}`
    /// (`scope` also accepts `"LongForm"` / `{"Kinds":[1,30023]}`). The kernel
    /// re-runs NIP-50 bounded-query validation, resolves the effective relay set
    /// from the active account's kind:10007 list, scans the local cache FTS
    /// scopes, and registers ONE typed `N50S` result sidecar under the
    /// per-session key `nmp.nip50.search.<sessionID>`.
    ///
    /// Fire-and-forget; an invalid query / malformed JSON is a kernel no-op (D6).
    func openSearch(requestJSON: String, sessionID: String) {
        requestJSON.withCString { requestPtr in
            sessionID.withCString { sessionPtr in
                nmp_app_search_open(raw, requestPtr, sessionPtr)
            }
        }
    }

    /// Close a search session opened with `openSearch`. Idempotent — pass the
    /// SAME `sessionID`. Unregisters the typed `N50S` sidecar and drops the
    /// relay-pinned interest. Call from the search sheet's `.onDisappear`.
    func closeSearch(sessionID: String) {
        sessionID.withCString { nmp_app_search_close(raw, $0) }
    }

    /// Pull the current typed `N50S` search-results buffer for `sessionID` via
    /// the two-call C size-probe (`nmp_app_search_snapshot`). Returns the raw
    /// FlatBuffers bytes, or `nil` when the session is unknown / has no data.
    ///
    /// This is the single-session pull seam the C-ABI exposes for hosts that
    /// read one search session rather than diffing whole snapshot frames — the
    /// `SearchController` drives it off the kernel's per-frame `objectWillChange`
    /// tick (reactive, never a timer/poll).
    func searchSnapshotBytes(sessionID: String) -> Data? {
        sessionID.withCString { sessionPtr -> Data? in
            // First call: size probe (out_buf = nil, cap = 0) → required length.
            let needed = nmp_app_search_snapshot(raw, sessionPtr, nil, 0)
            guard needed > 0 else { return nil }
            let count = Int(needed)
            var buffer = [UInt8](repeating: 0, count: count)
            // Second call: copy into a buffer of the probed size. `cap` is the
            // C `uintptr_t` parameter (imported as `UInt`).
            let written = buffer.withUnsafeMutableBufferPointer { ptr in
                nmp_app_search_snapshot(raw, sessionPtr, ptr.baseAddress, UInt(count))
            }
            guard Int(written) == count else { return nil }
            return Data(buffer)
        }
    }
}
