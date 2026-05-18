import SwiftUI

/// T156 — NmpPodcast app entry.
///
/// Mirrors the canonical Swift app at
/// `/Users/pablofernandez/src/podcast/PodcastApp/App/PodcastApp.swift` in
/// *shape* but binds to the kernel instead of a SwiftData `ModelContainer`.
/// All state — the library, subscription metadata, future episode/transcript
/// records — lives in Rust behind `KernelModel` (which composes
/// `nmp_app_new` + `nmp_app_podcast_register`).
///
/// Per the M11 design (`docs/design/podcast-app-rebuild.md` §1), every byte
/// rendered comes from a Rust DomainModule. SwiftData is forbidden.
@main
struct PodcastApp: App {
    @StateObject private var model = KernelModel()

    /// iOS scenePhase → kernel lifecycle bridge. Mirrors Chirp (D7).
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
                .task { model.start() }
        }
        .onChange(of: scenePhase) { _, newPhase in
            switch newPhase {
            case .active:
                model.lifecycleForeground()
            case .background:
                model.lifecycleBackground()
            case .inactive:
                break // transient — kernel never hears about it.
            @unknown default:
                break
            }
        }
    }
}
