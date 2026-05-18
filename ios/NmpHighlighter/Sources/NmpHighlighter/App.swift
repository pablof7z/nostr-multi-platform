import Kingfisher
import SwiftUI

@main
struct HighlighterApp: App {
    @State private var store = HighlighterStore()

    // MARK: - What's-new sheet wiring
    //
    // Evaluated once on cold launch. Uses `.sheet(item:)` rather than
    // `.sheet(isPresented:)` so the entries are bundled with the trigger —
    // avoids a SwiftUI render-race where a `fullScreenCover` (onboarding)
    // sitting on top causes the sheet's content closure to read stale entries.
    @State private var whatsNewPresentation: WhatsNewPresentation?

    init() {
        Self.configureImageCache()
    }

    var body: some Scene {
        WindowGroup {
            RootSceneView()
                .environment(store)
                .task {
                    WhatsNewService.seedIfNeeded()
                    let unseen = WhatsNewService.unseenEntries(
                        lastSeenAt: WhatsNewService.lastSeenAt
                    )
                    if !unseen.isEmpty {
                        whatsNewPresentation = WhatsNewPresentation(entries: unseen)
                    }
                }
                .sheet(item: $whatsNewPresentation) { presentation in
                    WhatsNewSheet(entries: presentation.entries)
                }
                .onOpenURL { url in
                    if ShareURLScheme.isProcessShare(url) {
                        // Share Extension handoff — drain the App Group queue.
                        Task { await ShareQueueProcessor.drain(app: store) }
                        return
                    }
                    // highlighter://nip46 callback brings us back from a signer app.
                    // Nothing to do — the actual pairing happens on the relay
                    // subscription started in the login view.
                }
        }
    }

}

/// Bundles entries with the trigger so the `.sheet(item:)` content closure
/// receives them atomically — see the wiring note above.
private struct WhatsNewPresentation: Identifiable {
    let id = UUID()
    let entries: [WhatsNewEntry]
}

extension HighlighterApp {
    private static func configureImageCache() {
        let cache = ImageCache.default
        cache.memoryStorage.config.totalCostLimit = 150 * 1024 * 1024   // 150 MB
        cache.memoryStorage.config.countLimit = 300
        cache.diskStorage.config.sizeLimit = 750 * 1024 * 1024          // 750 MB
        cache.diskStorage.config.expiration = .days(14)

        KingfisherManager.shared.downloader.downloadTimeout = 20
    }
}
