import SwiftUI

struct SetDetailView: View {
    @Environment(HighlighterStore.self) private var app
    let record: BookmarkSetRecord

    @State private var articles: [ArticleRecord] = []
    @State private var isLoading = false

    private var displayTitle: String {
        record.title.isEmpty ? (record.id.isEmpty ? "Collection" : record.id) : record.title
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
        Group {
            if isLoading {
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if articles.isEmpty && record.noteIds.isEmpty {
                ContentUnavailableView {
                    Label("Empty Collection", systemImage: "rectangle.stack")
                } description: {
                    Text("No items have been added to this collection yet.")
                }
            } else {
                articleList
            }
        }
        .navigationTitle(displayTitle)
        .navigationBarTitleDisplayMode(.large)
        .task { await loadArticles() }
        .task(id: record.pubkey) {
            await app.requestProfile(pubkeyHex: record.pubkey)
        }
    }

    private var curatorHeader: some View {
        HStack(spacing: 10) {
            AuthorAvatar(
                pubkey: record.pubkey,
                pictureURL: app.profileCache[record.pubkey]?.picture ?? "",
                displayInitial: curatorInitial,
                size: 32
            )
            VStack(alignment: .leading, spacing: 1) {
                Text("Curated by")
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(Color.highlighterInkMuted)
                    .textCase(.uppercase)
                    .tracking(0.6)
                Text(curatorName)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Color.highlighterAccent.opacity(0.06))
    }

    private var articleList: some View {
        ScrollView {
            LazyVStack(spacing: 0) {
                curatorHeader
                Divider()
                ForEach(articles, id: \.eventId) { article in
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

    private func loadArticles() async {
        isLoading = true
        defer { isLoading = false }

        var loaded: [ArticleRecord] = []
        for address in record.articleAddresses {
            let parts = address.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
            guard parts.count == 3 else { continue }
            let pubkey = String(parts[1])
            let dTag = String(parts[2])
            guard !pubkey.isEmpty, !dTag.isEmpty else { continue }
            if let article = try? await app.safeCore.getArticle(pubkeyHex: pubkey, dTag: dTag) {
                loaded.append(article)
            }
        }
        articles = loaded.sorted {
            ($0.publishedAt ?? $0.createdAt ?? 0) > ($1.publishedAt ?? $1.createdAt ?? 0)
        }
    }
}
