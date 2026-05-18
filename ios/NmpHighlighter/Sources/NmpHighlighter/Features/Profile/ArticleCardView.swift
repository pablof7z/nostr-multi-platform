import Kingfisher
import SwiftUI

/// Article row on a profile's Writing tab. Mirrors the web `ArticleCard`:
/// title + summary (2-line clamp) on the left, 96×72 thumbnail on the
/// right, metadata row underneath.
struct ArticleCardView: View {
    let article: ArticleRecord

    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            VStack(alignment: .leading, spacing: 8) {
                if !article.title.isEmpty {
                    Text(article.title)
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)
                        .lineLimit(3)
                } else {
                    Text("Untitled")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(Color.highlighterInkMuted)
                }

                if !article.summary.isEmpty {
                    Text(article.summary)
                        .font(.subheadline)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(2)
                }

                metadataRow
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            if let url = thumbnailURL {
                KFImage(url)
                    .placeholder { Color.highlighterRule.opacity(0.4) }
                    .fade(duration: 0.15)
                    .resizable()
                    .scaledToFill()
                    .frame(width: 96, height: 72)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
            }
        }
        .padding(.vertical, 14)
    }

    private var thumbnailURL: URL? {
        guard !article.image.isEmpty else { return nil }
        return URL(string: article.image)
    }

    private var metadataRow: some View {
        HStack(spacing: 10) {
            if let date = displayDate {
                Text(date)
            }
            if !article.hashtags.isEmpty {
                Text("·")
                    .foregroundStyle(Color.highlighterInkMuted)
                Text(article.hashtags.prefix(2).map { "#\($0)" }.joined(separator: " "))
                    .lineLimit(1)
            }
        }
        .font(.caption)
        .foregroundStyle(Color.highlighterInkMuted)
    }

    private var displayDate: String? {
        let seconds = article.publishedAt ?? article.createdAt ?? 0
        guard seconds > 0 else { return nil }
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        return formatter.string(from: date)
    }
}
