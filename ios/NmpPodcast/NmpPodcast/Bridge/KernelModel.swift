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

    private let handle: KernelHandle

    init() {
        self.handle = KernelHandle()
        self.handle.listen { [weak self] _json in
            // Update callback runs on the Rust listener thread; bounce to the
            // main actor so the @Published mutation is correctly serialised.
            Task { @MainActor [weak self] in
                self?.refresh()
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
}
