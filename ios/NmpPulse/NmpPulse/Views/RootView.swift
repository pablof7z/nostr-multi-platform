import SwiftUI

/// Top-level navigation. Pulse v0 is single-screen (Timeline) because the
/// onboarding / compose / accounts flows depend on FFI surface that is not
/// yet wired into the actor (filed as T66a). Each placeholder tab is wired
/// up so the navigation chrome is in place when the FFI surface lands.
struct RootView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        TabView {
            NavigationStack {
                TimelineView()
            }
            .tabItem {
                Label("Timeline", systemImage: "house")
            }

            NavigationStack {
                DiagnosticsView()
            }
            .tabItem {
                Label("Diagnostics", systemImage: "gauge")
            }

            NavigationStack {
                PendingFeaturesView()
            }
            .tabItem {
                Label("More", systemImage: "ellipsis.circle")
            }
        }
        .task {
            model.start()
        }
    }
}
