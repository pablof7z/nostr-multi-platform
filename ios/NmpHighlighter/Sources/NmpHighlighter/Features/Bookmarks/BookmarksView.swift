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

struct BookmarkedArticleRow: View {
    @Environment(HighlighterStore.self) private var app
    let article: ArticleRecord

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            coverImage
                .frame(width: 56, height: 56)
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))

            VStack(alignment: .leading, spacing: 4) {
                Text(article.title.isEmpty ? "Untitled" : article.title)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)

                if !article.summary.isEmpty {
                    Text(article.summary)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                }

                HStack(spacing: 4) {
                    Text(authorName)
                        .font(.caption2.weight(.medium))
                        .foregroundStyle(Color.highlighterInkMuted)
                    if let date = relativeDate {
                        Text("·")
                            .font(.caption2)
                            .foregroundStyle(Color.highlighterInkMuted)
                        Text(date)
                            .font(.caption2)
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
            }

            Spacer(minLength: 0)

            Image(systemName: "chevron.right")
                .font(.caption.weight(.semibold))
                .foregroundStyle(Color.highlighterInkMuted.opacity(0.5))
        }
        .task(id: article.pubkey) {
            await app.requestProfile(pubkeyHex: article.pubkey)
        }
    }

    @ViewBuilder
    private var coverImage: some View {
        if !article.image.isEmpty, let url = URL(string: article.image) {
            KFImage(url)
                .placeholder { coverFallback }
                .fade(duration: 0.15)
                .resizable()
                .scaledToFill()
        } else {
            coverFallback
        }
    }

    private var coverFallback: some View {
        ZStack {
            LinearGradient(
                colors: [Color.highlighterAccent.opacity(0.28), Color.highlighterAccent.opacity(0.10)],
                startPoint: .topLeading, endPoint: .bottomTrailing
            )
            Image(systemName: "doc.text")
                .font(.system(size: 20, weight: .medium))
                .foregroundStyle(Color.highlighterInkStrong.opacity(0.4))
        }
    }

    private var authorName: String {
        let profile = app.profileCache[article.pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(article.pubkey.prefix(10))
    }

    private var relativeDate: String? {
        let seconds = article.publishedAt ?? article.createdAt
        guard let s = seconds, s > 0 else { return nil }
        let delta = Date().timeIntervalSince1970 - TimeInterval(s)
        guard delta >= 0 else { return nil }
        switch delta {
        case ..<3600:           return "\(Int(delta / 60))m"
        case ..<86400:          return "\(Int(delta / 3600))h"
        case ..<(86400 * 7):    return "\(Int(delta / 86400))d"
        case ..<(86400 * 30):   return "\(Int(delta / (86400 * 7)))w"
        default:                return "\(Int(delta / (86400 * 30)))mo"
        }
    }
}

struct CollectionRow: View {
    @Environment(HighlighterStore.self) private var app
    let record: BookmarkSetRecord

    private var displayTitle: String {
        record.title.isEmpty ? (record.id.isEmpty ? "Untitled" : record.id) : record.title
    }

    private var kindLabel: String {
        record.kind == 30003 ? "Bookmarks" : "Curation"
    }

    private var itemCount: Int {
        record.articleAddresses.count + record.noteIds.count
    }

    private var curatorName: String {
        let profile = app.profileCache[record.pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(record.pubkey.prefix(10))
    }

    private var curatorInitial: String {
        curatorName.first.map { String($0).uppercased() } ?? ""
    }

    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(Color.highlighterAccent.opacity(0.12))
                    .frame(width: 44, height: 44)
                Image(systemName: record.kind == 30003 ? "bookmark.fill" : "rectangle.stack.fill")
                    .font(.system(size: 18, weight: .medium))
                    .foregroundStyle(Color.highlighterAccent)
            }

            VStack(alignment: .leading, spacing: 4) {
                Text(displayTitle)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(1)

                HStack(spacing: 6) {
                    AuthorAvatar(
                        pubkey: record.pubkey,
                        pictureURL: app.profileCache[record.pubkey]?.picture ?? "",
                        displayInitial: curatorInitial,
                        size: 16
                    )
                    Text(curatorName)
                        .font(.caption2.weight(.medium))
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(1)
                }

                HStack(spacing: 4) {
                    Text(kindLabel)
                        .font(.caption2.weight(.medium))
                        .foregroundStyle(Color.highlighterAccent.opacity(0.8))
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(Color.highlighterAccent.opacity(0.1), in: Capsule())

                    if itemCount > 0 {
                        Text("\(itemCount) item\(itemCount == 1 ? "" : "s")")
                            .font(.caption2)
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
            }

            Spacer(minLength: 0)

            Image(systemName: "chevron.right")
                .font(.caption.weight(.semibold))
                .foregroundStyle(Color.highlighterInkMuted.opacity(0.5))
        }
        .task(id: record.pubkey) {
            await app.requestProfile(pubkeyHex: record.pubkey)
        }
    }
}

struct WebBookmarkRow: View {
    let bookmark: WebBookmarkRecord

    private var displayTitle: String {
        bookmark.title.isEmpty ? bookmark.url : bookmark.title
    }

    private var host: String? {
        URL(string: bookmark.url)?.host
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Image(systemName: "globe")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(Color.highlighterAccent)

                if let host {
                    Text(host)
                        .font(.caption2.weight(.medium))
                        .foregroundStyle(Color.highlighterInkMuted)
                }

                Spacer(minLength: 0)

                if let date = relativeDate {
                    Text(date)
                        .font(.caption2)
                        .foregroundStyle(Color.highlighterInkMuted)
                }
            }

            Text(displayTitle)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(Color.highlighterInkStrong)
                .lineLimit(2)
                .multilineTextAlignment(.leading)

            if !bookmark.description.isEmpty {
                Text(bookmark.description)
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
            }

            if !bookmark.topics.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 4) {
                        ForEach(bookmark.topics, id: \.self) { topic in
                            Text("#\(topic)")
                                .font(.caption2.weight(.medium))
                                .foregroundStyle(Color.highlighterAccent.opacity(0.8))
                                .padding(.horizontal, 6)
                                .padding(.vertical, 2)
                                .background(Color.highlighterAccent.opacity(0.1), in: Capsule())
                        }
                    }
                }
                .scrollClipDisabled()
            }
        }
    }

    private var relativeDate: String? {
        let seconds = bookmark.publishedAt ?? bookmark.createdAt
        guard let s = seconds, s > 0 else { return nil }
        let delta = Date().timeIntervalSince1970 - TimeInterval(s)
        guard delta >= 0 else { return nil }
        switch delta {
        case ..<3600:           return "\(Int(delta / 60))m"
        case ..<86400:          return "\(Int(delta / 3600))h"
        case ..<(86400 * 7):    return "\(Int(delta / 86400))d"
        case ..<(86400 * 30):   return "\(Int(delta / (86400 * 7)))w"
        default:                return "\(Int(delta / (86400 * 30)))mo"
        }
    }
}

