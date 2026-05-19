import SwiftUI

/// Rooms tab root. One continuous scrolling surface in the Apple TV "Home"
/// style: featured hero at the top, followed by editorial and social
/// shelves, then a Browse-all entry point at the bottom. No segmented
/// toggles — "Your rooms" is just the first shelf among many.
struct RoomExplorerView: View {
    @Environment(HighlighterStore.self) private var appStore
    @State private var explorer: RoomExplorerStore?
    @State private var previewRoom: CommunitySummary?
    @State private var createSheetPresented = false

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(spacing: 0, pinnedViews: []) {
                    heroSection
                        .padding(.bottom, 32)

                    yourRoomsShelf
                    friendsShelf
                    featuredShelf
                    authorsShelf
                    newShelf

                    browseAllFooter
                        .padding(.horizontal, 18)
                        .padding(.top, 28)
                        .padding(.bottom, 40)
                }
                .padding(.top, 4)
            }
            .background(Color.highlighterPaper.ignoresSafeArea())
            .navigationTitle("Rooms")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        createSheetPresented = true
                    } label: {
                        Image(systemName: "plus.circle")
                            .font(.title3)
                    }
                    .accessibilityLabel("New room")
                }
            }
            .navigationDestination(for: String.self) { groupId in
                RoomHomeView(groupId: groupId)
            }
            .navigationDestination(for: ProfileDestination.self) { destination in
                switch destination {
                case .pubkey(let pk):
                    ProfileView(pubkey: pk)
                }
            }
            .navigationDestination(for: ArticleReaderTarget.self) { target in
                ArticleReaderView(target: target)
            }
            .globalUserToolbar()
            .sheet(item: $previewRoom) { room in
                NavigationStack {
                    RoomPreviewSheet(
                        room: room,
                        onJoin: {
                            Task { await explorer?.requestJoin(room: room) }
                            previewRoom = nil
                        }
                    )
                }
                .environment(appStore)
            }
            .sheet(isPresented: $createSheetPresented) {
                CreateRoomSheet()
                    .environment(appStore)
                    .presentationDetents([.large])
            }
        }
        .task {
            if explorer == nil {
                explorer = RoomExplorerStore(appStore: appStore)
            }
            if let e = explorer {
                appStore.eventBridge?.registerExplorer(e)
            }
            await explorer?.refresh()
        }
        .refreshable {
            await explorer?.refresh()
        }
    }

    // MARK: - Sections

    @ViewBuilder
    private var heroSection: some View {
        if let featured = explorer?.featured, !featured.isEmpty {
            ExplorerHeroView(rooms: featured) { room in
                previewRoom = room
            }
            .padding(.top, 4)
        } else if explorer?.isFirstLoad == true {
            ExplorerHeroPlaceholder()
                .padding(.top, 4)
        }
    }

    @ViewBuilder
    private var yourRoomsShelf: some View {
        if !appStore.joinedCommunities.isEmpty {
            VStack(alignment: .leading, spacing: 12) {
                shelfTitle("Your rooms", rationale: nil)

                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(alignment: .top, spacing: 14) {
                        ForEach(appStore.joinedCommunities, id: \.id) { room in
                            NavigationLink(value: room.id) {
                                RoomCoverCard(room: room, width: 140)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 18)
                }
            }
            .padding(.bottom, 28)
        }
    }

    @ViewBuilder
    private var friendsShelf: some View {
        if let friends = explorer?.friendsShelf, !friends.isEmpty {
            shelf(
                title: "Friends are here",
                rationale: "People you follow are members",
                content: {
                    ForEach(friends, id: \.summary.id) { rec in
                        Button {
                            previewRoom = rec.summary
                        } label: {
                            FriendsOnRoomCard(recommendation: rec)
                        }
                        .buttonStyle(.plain)
                    }
                }
            )
        }
    }

    @ViewBuilder
    private var featuredShelf: some View {
        if let featured = explorer?.featured, featured.count > 1 {
            // After the hero, show the rest of the featured list as a
            // regular-sized shelf so the curator's full picks remain
            // accessible below the hero.
            shelf(
                title: "Featured",
                rationale: "Curated by Highlighter",
                content: {
                    ForEach(Array(featured.dropFirst()), id: \.id) { room in
                        Button {
                            previewRoom = room
                        } label: {
                            RoomSquareTile(room: room)
                        }
                        .buttonStyle(.plain)
                    }
                }
            )
        }
    }

    @ViewBuilder
    private var authorsShelf: some View {
        if let authors = explorer?.authorsShelf, !authors.isEmpty {
            shelf(
                title: "Writers you read",
                rationale: "Authors you've highlighted post here",
                content: {
                    ForEach(authors, id: \.summary.id) { rec in
                        Button {
                            previewRoom = rec.summary
                        } label: {
                            RoomSquareTile(room: rec.summary)
                        }
                        .buttonStyle(.plain)
                    }
                }
            )
        }
    }

    @ViewBuilder
    private var newShelf: some View {
        if let new = explorer?.newNoteworthy, !new.isEmpty {
            shelf(
                title: "New & noteworthy",
                rationale: "Recently added rooms",
                content: {
                    ForEach(new, id: \.id) { room in
                        Button {
                            previewRoom = room
                        } label: {
                            RoomSquareTile(room: room)
                        }
                        .buttonStyle(.plain)
                    }
                }
            )
        }
    }

    private var browseAllFooter: some View {
        NavigationLink {
            RoomBrowseAllView()
        } label: {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Browse all rooms")
                        .font(.body.weight(.medium))
                        .foregroundStyle(Color.highlighterInkStrong)
                    Text("The full catalog, searchable")
                        .font(.footnote)
                        .foregroundStyle(Color.highlighterInkMuted)
                }
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 18)
            .background(
                RoundedRectangle(cornerRadius: 14)
                    .stroke(Color.highlighterRule, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }

    // MARK: - Shelf shell

    private func shelfTitle(_ title: String, rationale: String?) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title.uppercased())
                .font(.footnote.weight(.semibold))
                .tracking(1.2)
                .foregroundStyle(Color.highlighterInkMuted)
            if let rationale {
                Text(rationale)
                    .font(.subheadline)
                    .foregroundStyle(Color.highlighterInkStrong)
            }
        }
        .padding(.horizontal, 18)
    }

    @ViewBuilder
    private func shelf<Content: View>(
        title: String,
        rationale: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            shelfTitle(title, rationale: rationale)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(alignment: .top, spacing: 14) {
                    content()
                }
                .padding(.horizontal, 18)
            }
        }
        .padding(.bottom, 28)
    }
}

// MARK: - CommunitySummary + Identifiable

extension CommunitySummary: Identifiable {}

// MARK: - Placeholder

private struct ExplorerHeroPlaceholder: View {
    var body: some View {
        RoundedRectangle(cornerRadius: 20)
            .fill(Color.highlighterRule.opacity(0.4))
            .frame(height: 260)
            .padding(.horizontal, 18)
            .shimmer()
    }
}

// MARK: - Simple shimmer

private struct ShimmerModifier: ViewModifier {
    @State private var phase: CGFloat = -1

    func body(content: Content) -> some View {
        content
            .overlay(
                LinearGradient(
                    colors: [.clear, Color.white.opacity(0.25), .clear],
                    startPoint: .leading,
                    endPoint: .trailing
                )
                .rotationEffect(.degrees(20))
                .offset(x: phase * 400)
                .blendMode(.plusLighter)
                .mask(content)
            )
            .onAppear {
                withAnimation(.linear(duration: 1.4).repeatForever(autoreverses: false)) {
                    phase = 1.5
                }
            }
    }
}

private extension View {
    func shimmer() -> some View { modifier(ShimmerModifier()) }
}
