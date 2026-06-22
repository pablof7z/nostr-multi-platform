import Combine
import Foundation

/// Self-contained driver for one NIP-50 search session.
///
/// `SearchSheet` holds one of these as a `@StateObject`. It owns the session id,
/// forwards the user's query to the kernel via the `open_search` C-ABI, and
/// surfaces the kernel's typed `N50S` result projection as `hits` — pulled via
/// the single-session `nmp_app_search_snapshot` size-probe seam.
///
/// REACTIVE, NOT POLLED: the controller subscribes to the kernel model's
/// `objectWillChange` (which fires once per applied snapshot frame, ADR-0055)
/// and re-pulls the session buffer on that signal. There is no timer / sleep
/// loop (D8). When a frame carries a fresh `N50S` buffer for this session, the
/// pull returns the new bytes and `hits` publishes; otherwise the decoded list
/// is unchanged.
///
/// THIN SHELL: this object holds NO search logic. It only (1) serializes the
/// request JSON, (2) calls C-ABI passthroughs on `KernelHandle`, and (3) decodes
/// the `N50S` FlatBuffers via the generated bindings (`SearchResultsDecoder`).
/// The kernel owns query validation, relay selection, cache-FTS, dedup, and
/// ordering.
@MainActor
final class SearchController: ObservableObject {
    /// The kernel-ordered (newest-first, id-stable), deduplicated hits for the
    /// live session. Empty until the first `N50S` frame for the session lands.
    @Published private(set) var hits: [ChirpSearchHit] = []
    /// The query currently running (set on `runSearch`). Drives the sheet's
    /// empty-state copy ("prompt" vs "no matches"). `nil` ⇒ nothing submitted.
    @Published private(set) var submittedQuery: String?

    /// Stable per-controller session id. Keys the kernel's
    /// `nmp.nip50.search.<id>` sidecar + the matching `closeSearch`.
    private let sessionID = "chirp.search.\(UUID().uuidString)"
    private weak var kernel: KernelHandle?
    /// Subscription to the model's per-frame change signal; held so the pull
    /// stays live for the controller's lifetime, torn down on `close()`/dealloc.
    private var cancellable: AnyCancellable?

    /// Bind to the kernel model. Subscribes to its per-frame `objectWillChange`
    /// so each applied snapshot re-pulls this session's `N50S` buffer. Idempotent.
    func bind(to model: KernelModel) {
        guard kernel == nil else { return }
        kernel = model.kernel
        cancellable = model.objectWillChange
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in self?.refresh() }
    }

    /// Serialize a NIP-50 `SearchRequest` and open the session over the C-ABI.
    /// `scopeJSON` is the serde value the Rust `SearchScope` enum accepts
    /// (`"Users"` or `{"Kinds":[1]}`). The kernel re-validates the bounded query
    /// and caps `max_hits`, so this side carries no authority.
    func runSearch(query: String, scopeJSON: Any) {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        submittedQuery = trimmed
        hits = []
        let payload: [String: Any] = [
            "query": trimmed,
            "scope": scopeJSON,
            "targets": "UserPreferred",
            "max_hits": 50,
        ]
        guard JSONSerialization.isValidJSONObject(payload),
              let data = try? JSONSerialization.data(withJSONObject: payload),
              let json = String(data: data, encoding: .utf8) else { return }
        kernel?.openSearch(requestJSON: json, sessionID: sessionID)
        // Pull immediately in case a cache-FTS hit is already available this tick.
        refresh()
    }

    /// Close the session (idempotent). Call from the sheet's `.onDisappear`.
    func close() {
        cancellable = nil
        kernel?.closeSearch(sessionID: sessionID)
    }

    /// Pull + decode this session's current `N50S` buffer, publishing `hits`
    /// only when the decoded list actually changed (avoids redundant SwiftUI
    /// invalidation on frames that didn't touch this session).
    private func refresh() {
        guard submittedQuery != nil, let kernel else { return }
        guard let bytes = kernel.searchSnapshotBytes(sessionID: sessionID) else { return }
        let decoded = SearchResultsDecoder.decode(bytes: bytes)
        if decoded != hits { hits = decoded }
    }
}
