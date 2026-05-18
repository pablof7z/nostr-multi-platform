import Kingfisher
import SwiftUI

enum BookmarkFilter: CaseIterable, Identifiable {
    case articles, collections, web

    var id: Self { self }

    var label: String {
        switch self {
        case .articles:    return "Articles"
        case .collections: return "Collections"
        case .web:         return "Web"
        }
    }

    var icon: String {
        switch self {
        case .articles:    return "doc.text"
        case .collections: return "rectangle.stack"
        case .web:         return "globe"
        }
    }
}

struct BookmarksView: View {
    @Environment(HighlighterStore.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var store = BookmarkStore()
    @State private var filter: BookmarkFilter = .articles

    var body: some View {
        NavigationStack {
            Group {
                if store.isLoading && store.myArticles.isEmpty && store.myBookmarkSets.isEmpty {
                    ProgressView()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    scrollContent
                }
            }
            .background(Color.highlighterPaper.ignoresSafeArea())
            .navigationTitle("Bookmarks")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    scopePicker
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .navigationDestination(for: ArticleReaderTarget.self) { target in
                ArticleReaderView(target: target)
            }
            .navigationDestination(for: BookmarkSetRecord.self) { rec in
                SetDetailView(record: rec)
            }
        }
        .task(id: app.bookmarkedArticleAddresses) {
            guard let bridge = app.eventBridge else { return }
            await store.start(
                addresses: app.bookmarkedArticleAddresses,
                core: app.safeCore,
                bridge: bridge
            )
        }
        .onDisappear { store.stop() }
    }

    private var scopePicker: some View {
        Picker("Scope", selection: $store.scope) {
            Text("Mine").tag(BookmarkScope.mine)
            Text("Explore").tag(BookmarkScope.explore)
        }
        .pickerStyle(.segmented)
        .fixedSize()
    }

    private var scrollContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                if store.scope == .mine {
                    filterChipRail
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                    Divider()
                    mineContent
                } else {
                    exploreContent
                        .padding(.top, 16)
                }
            }
        }
    }

    private var filterChipRail: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(BookmarkFilter.allCases) { item in
                    chip(for: item)
                }
            }
        }
        .scrollClipDisabled()
    }

    private func chip(for item: BookmarkFilter) -> some View {
        let isActive = filter == item
        return Button {
            withAnimation(.spring(duration: 0.22)) { filter = item }
        } label: {
            HStack(spacing: 5) {
                Image(systemName: item.icon)
                    .font(.caption.weight(.semibold))
                Text(item.label)
                    .font(.subheadline.weight(.medium))
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .foregroundStyle(isActive ? Color.highlighterAccent : Color.highlighterInkMuted)
            .background(.ultraThinMaterial, in: Capsule())
            .overlay(
                Capsule().strokeBorder(
                    isActive ? Color.highlighterAccent.opacity(0.4) : Color.highlighterRule,
                    lineWidth: 1
                )
            )
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder
    private var mineContent: some View {
        switch filter {
        case .articles:
            articlesContent
        case .collections:
            collectionsContent(
                sets: store.myBookmarkSets + store.myCurationSets,
                emptyTitle: "No collections yet",
                emptyMessage: "Create bookmark or curation sets to organise your saved content."
            )
        case .web:
            webContent
        }
    }

    @ViewBuilder
    private var articlesContent: some View {
        if store.myArticles.isEmpty {
            unavailableState(
                icon: "bookmark",
                title: "No bookmarks yet",
                message: "Save articles from anywhere in Highlighter to find them here."
            )
        } else {
            LazyVStack(spacing: 0) {
                ForEach(store.myArticles, id: \.eventId) { article in
                    NavigationLink(value: ArticleReaderTarget(pubkey: article.pubkey, dTag: article.identifier, seed: article)) {
                        BookmarkedArticleRow(article: article)
                            .padding(.horizontal, 16)
                            .padding(.vertical, 12)
                    }
                    .buttonStyle(.plain)
                    Divider().padding(.leading, 84)
                }
            }
        }
    }

    @ViewBuilder
    private var webContent: some View {
        if store.myWebBookmarks.isEmpty {
            unavailableState(
                icon: "globe",
                title: "No web bookmarks yet",
                message: "Web pages you bookmark via Nostr will appear here."
            )
        } else {
            LazyVStack(spacing: 0) {
                ForEach(store.myWebBookmarks, id: \.url) { bookmark in
                    WebBookmarkRow(bookmark: bookmark)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                    Divider().padding(.leading, 16)
                }
            }
        }
    }

    @ViewBuilder
    private var exploreContent: some View {
        if store.followingCurationSets.isEmpty {
            unavailableState(
                icon: "rectangle.stack",
                title: "Nothing to explore",
                message: "People you follow haven't created any curation sets yet."
            )
        } else {
            collectionsContent(
                sets: store.followingCurationSets,
                emptyTitle: "Nothing to explore",
                emptyMessage: "People you follow haven't created any curation sets yet."
            )
        }
    }

    @ViewBuilder
    private func collectionsContent(sets: [BookmarkSetRecord], emptyTitle: String, emptyMessage: String) -> some View {
        if sets.isEmpty {
            unavailableState(icon: "rectangle.stack", title: emptyTitle, message: emptyMessage)
        } else {
            LazyVStack(spacing: 0) {
                ForEach(sets, id: \.id) { set in
                    NavigationLink(value: set) {
                        CollectionRow(record: set)
                            .padding(.horizontal, 16)
                            .padding(.vertical, 12)
                    }
                    .buttonStyle(.plain)
                    Divider().padding(.leading, 16)
                }
            }
        }
    }

    private func unavailableState(icon: String, title: String, message: String) -> some View {
        ContentUnavailableView {
            Label(title, systemImage: icon)
        } description: {
            Text(message)
        }
        .padding(.top, 40)
    }
}

// MARK: - Row views

