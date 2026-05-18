import SwiftUI

struct RoomHomeView: View {
    enum Tab: Hashable { case home, library, discussions, chat }

    let groupId: String

    @Environment(HighlighterStore.self) private var app
    @State private var room = RoomStore()
    @State private var selectedTab: Tab = .home
    @State private var composerPresented: Bool = false
    @State private var suggestPresented: Bool = false
    @State private var capturePresented: Bool = false
    @State private var shareTarget: ShareToCommunityTarget?
    @State private var inviteSheetPresented: Bool = false
    @State private var hasChatActivity: Bool = false
    @State private var chatUnread: Bool = false
    @State private var chatPresenceProbe = ChatPresenceProbe()

    var body: some View {
        tabContent
            .navigationTitle(communityName)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar(.hidden, for: .tabBar)
            .safeAreaInset(edge: .bottom, spacing: 0) {
                pillTabBar
            }
            .navigationDestination(for: ArtifactRecord.self) { artifact in
                ArtifactDetailView(artifact: artifact)
            }
            .navigationDestination(for: DiscussionRecord.self) { discussion in
                DiscussionDetailView(discussion: discussion)
            }
            .toolbar {
                ToolbarItemGroup(placement: .topBarTrailing) {
                    if selectedTab == .home {
                        Button { capturePresented = true } label: {
                            Image(systemName: "camera")
                        }
                    } else if selectedTab == .library {
                        Button { suggestPresented = true } label: {
                            Image(systemName: "plus")
                        }
                    } else if selectedTab == .discussions {
                        Button { composerPresented = true } label: {
                            Image(systemName: "square.and.pencil")
                        }
                    }
                    Button { inviteSheetPresented = true } label: {
                        Image(systemName: "person.badge.plus")
                    }
                }
            }
            .onChange(of: selectedTab) { _, tab in
                if tab == .chat { chatUnread = false }
            }
            .task {
                await room.start(groupId: groupId, core: app.safeCore, bridge: app.eventBridge)
                await chatPresenceProbe.start(
                    groupId: groupId,
                    core: app.safeCore,
                    bridge: app.eventBridge,
                    onActivity: {
                        hasChatActivity = true
                        if selectedTab != .chat { chatUnread = true }
                    }
                )
            }
            .onDisappear {
                room.stop()
                chatPresenceProbe.stop()
                if selectedTab == .chat && !hasChatActivity {
                    selectedTab = .home
                }
            }
            .sheet(item: $shareTarget) { target in
                ShareToCommunitySheet(target: target)
                    .presentationDetents([.medium, .large])
            }
            .sheet(isPresented: $inviteSheetPresented) {
                NavigationStack {
                    RoomInviteView(groupId: groupId, mode: .manage, onClose: nil)
                }
                .environment(app)
                .presentationDetents([.large])
            }
            .captureFlow(isPresented: $capturePresented, preselectedGroupId: groupId)
            .sheet(isPresented: $suggestPresented) {
                DiscussionComposerView(
                    groupId: groupId,
                    navigationTitle: "Suggest an artifact"
                ) { _ in }
                .presentationDetents([.medium, .large])
            }
    }

    @ViewBuilder
    private var tabContent: some View {
        switch selectedTab {
        case .home:
            homeContent
        case .library:
            libraryContent
        case .discussions:
            DiscussionListView(groupId: groupId, composerPresented: $composerPresented)
        case .chat:
            ChatView(groupId: groupId)
        }
    }

    // MARK: - Pill tab bar

    private var pillTabBar: some View {
        HStack(spacing: 2) {
            pillSegment(.home, label: "Home")
            pillSegment(.library, label: "Library")
            pillSegment(.discussions, label: "Discussions")
            if hasChatActivity {
                pillSegment(.chat, label: "Chat", badge: chatUnread)
            }
        }
        .padding(4)
        .background(.ultraThinMaterial, in: Capsule())
        .shadow(color: .black.opacity(0.12), radius: 12, y: 4)
        .padding(.horizontal, 24)
        .padding(.bottom, 12)
        .animation(.spring(response: 0.3, dampingFraction: 0.75), value: hasChatActivity)
    }

    private func pillSegment(_ tab: Tab, label: String, badge: Bool = false) -> some View {
        Button {
            selectedTab = tab
        } label: {
            Text(label)
                .font(.subheadline.weight(selectedTab == tab ? .semibold : .regular))
                .foregroundStyle(selectedTab == tab ? Color.primary : Color.secondary)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background {
                    if selectedTab == tab {
                        Capsule()
                            .fill(Color(.systemBackground))
                            .shadow(color: .black.opacity(0.08), radius: 4, y: 1)
                    }
                }
                .overlay(alignment: .topTrailing) {
                    if badge {
                        Circle()
                            .fill(Color.accentColor)
                            .frame(width: 7, height: 7)
                            .offset(x: 2, y: -1)
                    }
                }
        }
        .buttonStyle(.plain)
        .animation(.spring(response: 0.3, dampingFraction: 0.75), value: selectedTab)
    }

    // MARK: - Home tab

    private var homeContent: some View {
        RoomLanesView(
            artifacts: room.artifacts,
            highlights: room.highlights,
            highlightsByReference: room.highlightsByReference,
            commentsByReference: room.commentsByReference,
            isLoading: room.isLoading,
            onShareToCommunity: { artifact in
                shareTarget = .artifact(artifact)
            }
        )
    }

    // MARK: - Library tab

    @ViewBuilder
    private var libraryContent: some View {
        if room.isLoading && room.artifacts.isEmpty && room.highlights.isEmpty {
            ProgressView().controlSize(.large)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if room.artifacts.isEmpty && room.highlights.isEmpty {
            ContentUnavailableView(
                "Nothing here yet",
                systemImage: "square.stack.3d.up",
                description: Text("Shares and highlights will appear as activity flows in.")
            )
        } else {
            ScrollView {
                LazyVStack(spacing: 0) {
                    if !room.artifacts.isEmpty {
                        ForEach(Array(room.artifacts.enumerated()), id: \.element.shareEventId) { index, a in
                            NavigationLink(value: a) {
                                artifactRow(a, commentCount: commentCount(for: a))
                            }
                            .buttonStyle(.plain)
                            .contextMenu {
                                Button {
                                    shareTarget = .artifact(a)
                                } label: {
                                    Label("Share to community", systemImage: "square.and.arrow.up")
                                }
                            }

                            if index < room.artifacts.count - 1 {
                                Rectangle()
                                    .fill(Color.highlighterRule)
                                    .frame(height: 1)
                            }
                        }
                    }

                    if !room.highlights.isEmpty {
                        highlightsSection
                    }
                }
                .padding(.horizontal, 20)
            }
            .background(Color.highlighterPaper.ignoresSafeArea())
        }
    }

    @ViewBuilder
    private func artifactRow(_ a: ArtifactRecord, commentCount: Int) -> some View {
        switch a.preview.source {
        case "article":
            RoomLibraryArticleCardView(artifact: a, commentCount: commentCount)
        case "book":
            RoomLibraryBookCardView(artifact: a, commentCount: commentCount)
        case "podcast":
            RoomLibraryPodcastCardView(artifact: a, commentCount: commentCount)
        default:
            HStack {
                Text(a.preview.title.isEmpty ? "Untitled" : a.preview.title)
                    .foregroundStyle(Color.highlighterInkStrong)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.footnote)
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            .padding(.vertical, 14)
        }
    }

    /// Resolve the count of NIP-22 comments anchored to an artifact, using
    /// the same uppercase reference key convention as `Lane.build`.
    private func commentCount(for artifact: ArtifactRecord) -> Int {
        let pv = artifact.preview
        let upperTag: String
        let value: String
        if !pv.referenceTagName.isEmpty, !pv.referenceTagValue.isEmpty {
            upperTag = pv.referenceTagName.uppercased()
            value = pv.referenceTagValue
        } else if !pv.highlightTagName.isEmpty, !pv.highlightTagValue.isEmpty {
            upperTag = pv.highlightTagName.uppercased()
            value = pv.highlightTagValue
        } else {
            return 0
        }
        return room.commentsByReference["\(upperTag):\(value)"]?.count ?? 0
    }

    @ViewBuilder
    private var highlightsSection: some View {
        if !room.artifacts.isEmpty {
            Rectangle()
                .fill(Color.highlighterRule)
                .frame(height: 1)
        }

        Text("Highlights")
            .font(.footnote.weight(.semibold))
            .foregroundStyle(Color.highlighterInkMuted)
            .textCase(.uppercase)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.top, 18)
            .padding(.bottom, 8)

        ForEach(Array(room.highlights.enumerated()), id: \.element.highlight.eventId) { index, h in
            Text(h.highlight.quote)
                .lineLimit(3)
                .foregroundStyle(Color.highlighterInkStrong)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 14)

            if index < room.highlights.count - 1 {
                Rectangle()
                    .fill(Color.highlighterRule)
                    .frame(height: 1)
            }
        }
    }

    // MARK: - Header

    private var communityName: String {
        let match = app.joinedCommunities.first { $0.id == groupId }
        if let name = match?.name, !name.isEmpty { return name }
        return "Community"
    }
}
