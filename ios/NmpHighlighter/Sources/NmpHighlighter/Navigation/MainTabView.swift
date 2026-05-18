import SwiftUI

struct MainTabView: View {
    enum Section: Hashable {
        case highlights, rooms, search
    }

    @Environment(HighlighterStore.self) private var store
    @State private var selection: Section = .highlights

    var body: some View {
        TabView(selection: $selection) {
            Tab("Highlights", systemImage: "text.quote", value: Section.highlights) {
                HighlightsTabView()
            }
            Tab("Rooms", systemImage: "square.grid.2x2", value: Section.rooms) {
                RoomExplorerView()
            }
            // iOS 26 TabRole.search renders this as a distinct liquid-glass
            // capsule separated from the main tab bar.
            Tab(value: Section.search, role: .search) {
                SearchView()
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
        .tabViewBottomAccessory(isEnabled: store.podcastPlayer.currentArtifact != nil) {
            MiniPlayerView()
        }
    }
}
