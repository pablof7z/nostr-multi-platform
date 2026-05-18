import SwiftUI

/// Dense 2-column grid of every room cached locally. Search filters by name
/// and description. Reached from the Explorer home via the "Browse all
/// rooms" footer.
struct RoomBrowseAllView: View {
    @Environment(HighlighterStore.self) private var appStore

    @State private var rooms: [CommunitySummary] = []
    @State private var search: String = ""
    @State private var previewRoom: CommunitySummary?

    private let columns = [
        GridItem(.flexible(), spacing: 14),
        GridItem(.flexible(), spacing: 14),
    ]

    private var visible: [CommunitySummary] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return rooms }
        return rooms.filter {
            $0.name.lowercased().contains(q) || $0.about.lowercased().contains(q)
        }
    }

    var body: some View {
        ScrollView {
            LazyVGrid(columns: columns, spacing: 18) {
                ForEach(visible, id: \.id) { room in
                    Button {
                        previewRoom = room
                    } label: {
                        RoomCoverCard(room: room)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(18)
        }
        .background(Color.highlighterPaper.ignoresSafeArea())
        .navigationTitle("Browse rooms")
        .navigationBarTitleDisplayMode(.inline)
        .searchable(text: $search, placement: .navigationBarDrawer(displayMode: .always))
        .task {
            await appStore.safeCore.startRoomDiscovery()
            rooms = (try? await appStore.safeCore.getAllRooms(limit: 200)) ?? []
        }
        .refreshable {
            rooms = (try? await appStore.safeCore.getAllRooms(limit: 200)) ?? []
        }
        .sheet(item: $previewRoom) { room in
            NavigationStack {
                RoomPreviewSheet(
                    room: room,
                    onJoin: {
                        Task {
                            appStore.noteJoinRequested(groupId: room.id, roomName: room.name)
                            _ = try? await appStore.safeCore.requestJoinRoom(groupId: room.id)
                        }
                        previewRoom = nil
                    }
                )
            }
            .environment(appStore)
        }
    }
}
