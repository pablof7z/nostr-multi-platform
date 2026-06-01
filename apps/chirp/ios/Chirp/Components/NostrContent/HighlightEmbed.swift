import SwiftUI

/// Richer renderer for kind:9802 highlights (NIP-84).
///
/// Replaces `DefaultHighlightRenderer` via `registry.setHighlight(...)`.
/// Renders:
///   • highlighted text styled like a pull-quote (italic, left-accent stripe,
///     subtle background)
///   • optional `context` line beneath the highlight (de-emphasized)
///   • source footer: `r` URL link, `e` event id chip, or `a` address chip
///   • author byline (avatar + display name)
///
/// Mirrors the TUI's highlight renderer in
/// `crates/nmp-cli/registry/tui/content-kind-9802/`.
struct HighlightEmbed: KindRenderer {
    init() {}

    func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView {
        guard case .highlight(let highlight) = projection else {
            return AnyView(EmptyView())
        }

        let author = highlight.authorDisplayName ?? shortHex(highlight.authorPubkey)

        return AnyView(
            VStack(alignment: .leading, spacing: 10) {
                // Pull-quote: italic body with a thin yellow accent stripe.
                HStack(alignment: .top, spacing: 10) {
                    RoundedRectangle(cornerRadius: 1.5)
                        .fill(Color.yellow.opacity(0.7))
                        .frame(width: 3)
                    VStack(alignment: .leading, spacing: 6) {
                        Text("\u{201C}\(highlight.highlightedText)\u{201D}")
                            .font(.body.italic())
                            .foregroundStyle(.primary)
                            .fixedSize(horizontal: false, vertical: true)
                        if let context = highlight.context?.trimmingCharacters(in: .whitespacesAndNewlines),
                           !context.isEmpty
                        {
                            Text(context)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(8)
                .background(Color.yellow.opacity(0.06), in: RoundedRectangle(cornerRadius: 6))

                // Source footer.
                sourceFooter(highlight: highlight)

                // Author byline.
                HStack(spacing: 8) {
                    Image(systemName: "highlighter")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text("highlighted by \(author)")
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    Text("kind:9802")
                        .font(.caption2.monospaced())
                        .foregroundStyle(.tertiary)
                }
            }
        )
    }

    @ViewBuilder
    private func sourceFooter(highlight: HighlightProjection) -> some View {
        if let url = highlight.sourceUrl, !url.isEmpty {
            Label {
                Text(url)
                    .lineLimit(1)
                    .truncationMode(.middle)
            } icon: {
                Image(systemName: "link")
            }
            .font(.caption.monospaced())
            .foregroundStyle(.tint)
        } else if let eventId = highlight.sourceEventId, !eventId.isEmpty {
            Label("note · \(shortHex(eventId))", systemImage: "doc.text")
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
        } else if let addr = highlight.sourceEventAddr, !addr.isEmpty {
            Label(addr, systemImage: "doc.richtext")
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
    }

    private func shortHex(_ value: String) -> String {
        guard value.count > 10 else { return value }
        return "\(value.prefix(8))…"
    }
}
