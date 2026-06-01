import SwiftUI

private struct TappedImage: Identifiable {
    let url: URL
    var id: String { url.absoluteString }
}

struct NoteContentView: View {
    let content: String
    let contentTree: ContentTreeWire?
    let renderContext: NoteRenderContext
    var font: Font = .body

    @EnvironmentObject private var router: ChirpRouter
    @State private var tappedImage: TappedImage?

    init(
        content: String,
        contentTree: ContentTreeWire? = nil,
        mentionProfiles: [String: MentionProfile] = [:],
        eventCards: [String: ChirpEventCard] = [:],
        timelineItems: [String: TimelineItem] = [:],
        renderContext: NoteRenderContext? = nil,
        font: Font = .body
    ) {
        self.content = content
        self.contentTree = contentTree
        self.renderContext = renderContext ?? NoteRenderContext(
            mentionProfiles: mentionProfiles,
            eventCards: eventCards,
            timelineItems: timelineItems
        )
        self.font = font
    }

    var body: some View {
        Group {
            if let contentTree {
                richBody(contentTree)
            } else {
                plainBody
            }
        }
        .fullScreenCover(item: $tappedImage) { item in
            FullScreenImageViewer(url: item.url)
        }
    }

    @ViewBuilder
    private func richBody(_ tree: ContentTreeWire) -> some View {
        NostrContentView(
            tree: tree,
            font: font,
            mentionLabel: { uri in renderContext.mentionLabel(for: uri.primaryId) },
            quoteCardProvider: quoteCardModel(for:)
        )
        .nostrContentRenderer(chirpContentRenderer)
    }

    private var plainBody: some View {
        Text(content)
            .font(font)
    }

    private var chirpContentRenderer: NostrContentRenderer {
        NostrContentRenderer(
            textColor: .primary,
            secondaryTextColor: .secondary,
            mentionColor: ChirpColor.link,
            hashtagColor: ChirpColor.link,
            linkColor: ChirpColor.link,
            quoteBorderColor: ChirpColor.hairline.opacity(0.55),
            quoteBackgroundColor: ChirpColor.surface.opacity(0.75),
            codeBackgroundColor: ChirpColor.secondaryFill,
            placeholderColor: .secondary,
            callbacks: NostrContentCallbacks(
                onImageTap: { url in tappedImage = TappedImage(url: url) },
                onEventRefTap: { eventID in router.push(.thread(eventID: eventID)) }
            )
        )
    }

    private func quoteCardModel(for uri: NostrWireUri) -> NostrQuoteCardModel? {
        let eventID = uri.primaryId
        if let card = renderContext.eventCards[eventID] {
            return NostrQuoteCardModel(
                id: card.id,
                unresolvedUri: uri.uri,
                authorPubkey: card.authorPubkey,
                authorDisplayName: card.authorDisplayName,
                authorAvatarUrl: httpImageURL(card.authorPictureUrl),
                content: card.contentPreview.isEmpty ? card.content : card.contentPreview,
                createdAtDisplay: card.createdAt.relativeTimeFromUnixSeconds
            )
        }

        if let item = renderContext.timelineItems[eventID] {
            return NostrQuoteCardModel(
                id: item.id,
                unresolvedUri: uri.uri,
                authorPubkey: item.authorPubkey,
                authorDisplayName: renderContext.mentionLabel(for: item.authorPubkey),
                authorAvatarUrl: httpImageURL(item.authorPictureUrl),
                content: item.contentPreview.isEmpty ? item.renderedContent : item.contentPreview,
                createdAtDisplay: item.createdAt.relativeTimeFromUnixSeconds
            )
        }

        return nil
    }

    private func httpImageURL(_ value: String?) -> URL? {
        guard
            let value,
            let url = URL(string: value),
            let scheme = url.scheme?.lowercased(),
            ["http", "https"].contains(scheme)
        else {
            return nil
        }
        return url
    }
}

private struct FullScreenImageViewer: View {
    let url: URL
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ZStack(alignment: .topTrailing) {
            ChirpColor.mediaBackdrop.ignoresSafeArea()
            AsyncImage(url: url) { phase in
                if let img = phase.image {
                    img.resizable()
                        .scaledToFit()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if phase.error != nil {
                    VStack(spacing: 12) {
                        Image(systemName: "photo.badge.exclamationmark")
                            .font(.system(size: 48, weight: .light))
                        Text("Image unavailable")
                            .font(.callout)
                    }
                    .foregroundStyle(.secondary)
                } else {
                    ProgressView().tint(ChirpColor.mediaForeground)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .font(.title)
                    .symbolRenderingMode(.palette)
                    .foregroundStyle(ChirpColor.mediaForeground, ChirpColor.mediaSecondaryForeground)
                    .padding(20)
            }
        }
        .onTapGesture { dismiss() }
    }
}
