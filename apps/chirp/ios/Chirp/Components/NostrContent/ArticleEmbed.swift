import SwiftUI

/// Richer renderer for kind:30023 long-form articles (NIP-23).
///
/// Replaces `DefaultArticleRenderer` via `registry.setArticle(...)`. Renders:
///   • optional hero image (full-width, 16:9 crop)
///   • article title (large, semibold)
///   • optional summary line
///   • author chip: avatar + display name, styled as a NIP-65 byline
///
/// Mirrors the TUI's article renderer in
/// `crates/nmp-cli/registry/tui/content-kind-30023/`.
struct ArticleEmbed: KindRenderer {
    init() {}

    func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView {
        guard case .article(let article) = projection else {
            return AnyView(EmptyView())
        }

        let title = article.title?.trimmingCharacters(in: .whitespacesAndNewlines)
        let summary = article.summary?.trimmingCharacters(in: .whitespacesAndNewlines)
        let heroUrl = article.heroImageUrl.flatMap { URL(string: $0) }

        return AnyView(
            VStack(alignment: .leading, spacing: 10) {
                if let heroUrl {
                    AsyncImage(url: heroUrl) { phase in
                        switch phase {
                        case .success(let image):
                            image.resizable().scaledToFill()
                        default:
                            Rectangle().fill(Color.secondary.opacity(0.15))
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .aspectRatio(16.0 / 9.0, contentMode: .fill)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .accessibilityHidden(true)
                }

                if let title, !title.isEmpty {
                    Text(title)
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(.primary)
                        .fixedSize(horizontal: false, vertical: true)
                }

                if let summary, !summary.isEmpty {
                    Text(summary)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }

                // Author byline.
                HStack(spacing: 8) {
                    NostrAvatar(
                        pubkey: article.authorPubkey,
                        url: article.authorPictureUrl,
                        size: 24
                    )
                    .equatable()
                    Text(article.authorDisplayName ?? shortHex(article.authorPubkey))
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    Text("article · kind:30023")
                        .font(.caption2.monospaced())
                        .foregroundStyle(.tertiary)
                }
            }
        )
    }

    private func shortHex(_ value: String) -> String {
        guard value.count > 10 else { return value }
        return "\(value.prefix(8))…"
    }
}
