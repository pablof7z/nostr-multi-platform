import SwiftUI

/// Root tab container. Until the user signs in we present `AuthView`;
/// after `isSignedIn` flips the timeline + compose tabs replace it.
struct ContentView: View {
    let bridge: NotesBridge

    var body: some View {
        if bridge.isSignedIn {
            TabView {
                TimelineView(bridge: bridge)
                    .tabItem { Label("Timeline", systemImage: "list.bullet") }
                ComposeView(bridge: bridge)
                    .tabItem { Label("Compose", systemImage: "square.and.pencil") }
            }
        } else {
            AuthView(bridge: bridge)
        }
    }
}
