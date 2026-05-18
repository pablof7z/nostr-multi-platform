import Kingfisher
import SwiftUI

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

