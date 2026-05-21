import Foundation
import SwiftUI

/// Observable model the SwiftUI views read from. Owns the `KernelHandle` and
/// exposes a single published `library` that mirrors what the Rust
/// `nmp-app-podcast` snapshot reports. Every UI mutation is funneled through
/// here, never through the FFI directly.
///
/// All state derived from Rust is pull-based for now: after each mutation
/// (`subscribe` / `unsubscribe`) we re-pull the snapshot. The kernel's
/// `set_update_callback` is wired so future reactive sources (RSS poll
/// completion, NIP-77 reconcile) can refresh the snapshot too.
@MainActor
final class KernelModel: ObservableObject {
    @Published private(set) var library: LibrarySnapshot = .empty
    @Published private(set) var lastErrorMessage: String?
    @Published private(set) var relays: [RelayEditRow] = []
    @Published private(set) var relayStatuses: [RelayKernelStatus] = []

    private let handle: KernelHandle

    init() {
        self.handle = KernelHandle()
        self.handle.listen { [weak self] json in
            // Update callback runs on the Rust listener thread; bounce to the
            // main actor so the @Published mutations are correctly serialised.
            let snapshot = KernelRelaySnapshot.decode(envelope: json)
            Task { @MainActor [weak self] in
                self?.refresh()
                if let snapshot {
                    self?.relays = snapshot.relayEditRows
                    self?.relayStatuses = snapshot.relayStatuses
                }
            }
        }
    }

    /// Boot the kernel actor and pull the initial library snapshot.
    func start() {
        handle.start()
        refresh()
    }

    /// Re-pull the snapshot. Called after every mutation and from the kernel
    /// update callback.
    func refresh() {
        guard let view = handle.podcastSnapshot() else {
            lastErrorMessage = "Could not load library snapshot"
            return
        }
        library = view
        lastErrorMessage = nil
    }

    func subscribe(feedURL: String, title: String? = nil, author: String? = nil) {
        let trimmed = feedURL.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        handle.podcastSubscribe(feedURL: trimmed, title: title, author: author)
        refresh()
    }

    func unsubscribe(podcastID: String) {
        handle.podcastUnsubscribe(podcastID: podcastID)
        refresh()
    }

    func lifecycleForeground() {
        handle.lifecycleForeground()
    }

    func lifecycleBackground() {
        handle.lifecycleBackground()
    }

    // MARK: - Relays

    func addRelay(url: String, read: Bool, write: Bool) {
        let trimmed = url.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        handle.addRelay(url: trimmed, role: Self.roleString(read: read, write: write))
    }

    func setRelayRoles(url: String, read: Bool, write: Bool) {
        // `add_relay` upserts on URL — re-adding with a new role updates the
        // existing row in place (commands::add_relay handles the match).
        handle.addRelay(url: url, role: Self.roleString(read: read, write: write))
    }

    func removeRelay(url: String) {
        handle.removeRelay(url: url)
    }

    func status(for url: String) -> RelayKernelStatus? {
        relayStatuses.first { $0.relayUrl == url }
    }

    private static func roleString(read: Bool, write: Bool) -> String {
        switch (read, write) {
        case (true, true):  return "both"
        case (true, false): return "read"
        case (false, true): return "write"
        case (false, false): return "read" // never disable both — default to read
        }
    }
}
