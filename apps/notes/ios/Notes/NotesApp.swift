import SwiftUI

/// Notes — second-app stateful spike. Composed on the same generic NMP
/// substrate seams as Chirp with zero new C-ABI symbols. See
/// `apps/notes/README.md` for the framework-thesis verdict.
@main
struct NotesApp: App {
    @Environment(\.scenePhase) private var scenePhase
    @State private var bridge = NotesBridge()

    var body: some Scene {
        WindowGroup {
            ContentView(bridge: bridge)
                .task { bridge.start() }
        }
        .onChange(of: scenePhase) { _, newPhase in
            switch newPhase {
            case .active: bridge.foreground()
            case .background: bridge.background()
            default: break
            }
        }
    }
}
