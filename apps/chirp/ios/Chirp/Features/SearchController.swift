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

    /// A navigation request produced by the omnibox when the resolver classified
    /// the input as a direct reference (`npub`/`note`/`nevent`/`naddr`). The view
    /// observes this and pushes the matching `ChirpRoute`, then clears it. `nil`
    /// ⇒ no pending navigation. Carries only the typed decoded target — the
    /// view maps it onto a route (no parsing).
    @Published var pendingNavigation: DecodedRefTarget?

    /// A short, non-echoing status line the omnibox shows for non-search
    /// outcomes (a NIP-05 lookup in flight, a secret-key refusal, an
    /// unsupported target). `nil` ⇒ nothing to show. NEVER contains the raw
    /// input — a secret is never echoed.
    @Published private(set) var omniboxNotice: String?

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

    /// OMNIBOX submit — route one untyped input through the input-intent
    /// resolver (#1804) and act on the typed outcome. THIN SHELL: all
    /// classification lives in Rust; this method only switches on the decoded
    /// variant and drives the matching presentation/navigation.
    ///
    /// * `TextQuery`  → the kernel already opened the search session under this
    ///   controller's `sessionID` (we pass it to dispatch), so we just enter the
    ///   results state and let the reactive `N50S` pull surface hits.
    /// * `DirectRef`  → decode the URI to its typed target and publish a
    ///   navigation request for the view to push.
    /// * `Nip05`      → the kernel kicked off the async reverse lookup; show a
    ///   "looking up" notice (the profile resolves through normal projections).
    /// * `RelayUrl` / `Registered` → not actionable in a microblog; show a
    ///   graceful "not supported" notice.
    /// * Rejections   → a safe inline notice; a secret-key input is refused with
    ///   NO echo of the typed text.
    func submitOmnibox(input: String, textScope: IntentScope) {
        let trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let kernel else { return }
        omniboxNotice = nil
        // `nostr.ref` is always requested so paste-to-navigate works regardless
        // of the free-text scope toggle. Only the SELECTED free-text scope is
        // requested, so the resolver's top free-text candidate is the one the
        // user chose (the classifier orders text candidates by recognizer
        // registration, not by request order — requesting a single text scope is
        // the clean way to pick it without any Swift-side reordering).
        let scopes: [IntentScope] = [.nostrRef, textScope]
        guard let outcome = kernel.dispatchIntent(input: trimmed, scopes: scopes, sessionID: sessionID) else {
            omniboxNotice = "Couldn't process that input."
            return
        }
        switch outcome {
        case .dispatched(.textQuery):
            // Kernel opened the search under our session; enter results state.
            submittedQuery = trimmed
            hits = []
            refresh()
        case .dispatched(.directRef(let uri)):
            submittedQuery = nil
            if let target = kernel.decodeRefTarget(uri: uri) {
                pendingNavigation = target
            } else {
                omniboxNotice = "Couldn't open that reference."
            }
        case .dispatched(.nip05(let identifier)):
            submittedQuery = nil
            omniboxNotice = "Looking up \(identifier)\u{2026}"
        case .dispatched(.relayURL), .dispatched(.registered):
            submittedQuery = nil
            omniboxNotice = "That kind of link isn't supported here."
        case .rejection(.secretLike):
            submittedQuery = nil
            // SECURITY: never echo the input — a secret key is refused silently
            // by value.
            omniboxNotice = "Secret keys aren't accepted."
        case .rejection(.unparseable):
            submittedQuery = nil
            omniboxNotice = "Couldn't recognize that input."
        case .rejection(.disallowedScope), .rejection(.unregisteredScope):
            submittedQuery = nil
            omniboxNotice = "That isn't searchable here."
        }
    }

    /// Clear the omnibox notice (e.g. when the query field changes).
    func clearNotice() { omniboxNotice = nil }

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
