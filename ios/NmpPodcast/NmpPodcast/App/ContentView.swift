import SwiftUI

/// T156 — Root `TabView`. The canonical Swift app has four tabs (Feed / Ask
/// / Insights / Library). For this iteration only the Library tab is wired
/// end-to-end through the kernel; the other tabs land in subsequent
/// iterations as their kernel `ViewModule` wrappers are implemented.
/// Filed as T-podcast-gap-002.
struct ContentView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var selectedTab: Tab = .library

    enum Tab {
        case feed
        case ask
        case insights
        case library
    }

    var body: some View {
        TabView(selection: $selectedTab) {
            PlaceholderTab(name: "Feed", systemImage: "list.bullet",
                           subtitle: "Wires up once FeedViewModule lands.")
                .tabItem { Label("Feed", systemImage: "list.bullet") }
                .tag(Tab.feed)

            PlaceholderTab(name: "Ask", systemImage: "bubble.left.and.bubble.right",
                           subtitle: "Wires up once AskQuestion action lands.")
                .tabItem { Label("Ask", systemImage: "bubble.left.and.bubble.right") }
                .tag(Tab.ask)

            PlaceholderTab(name: "Insights", systemImage: "lightbulb",
                           subtitle: "Wires up once InsightsViewModule lands.")
                .tabItem { Label("Insights", systemImage: "lightbulb") }
                .tag(Tab.insights)

            LibraryView()
                .tabItem { Label("Library", systemImage: "books.vertical") }
                .tag(Tab.library)
        }
    }
}

/// Inline placeholder tabs. Replaced one-by-one as each tab's
/// kernel-wrapped view ships.
private struct PlaceholderTab: View {
    let name: String
    let systemImage: String
    let subtitle: String

    var body: some View {
        NavigationStack {
            ContentUnavailableView(
                name,
                systemImage: systemImage,
                description: Text(subtitle)
            )
            .navigationTitle(name)
        }
    }
}
