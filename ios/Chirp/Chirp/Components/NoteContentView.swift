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
        // No quoteCardProvider — embed refs flow through the NostrKindRegistry
        // environment path (EmbeddedEvent) injected by ChirpApp. The legacy
        // quoteCardProvider closure is intentionally omitted so that quote
        // cards use the same kind-registry seam as article/highlight embeds
        // (resolves #1179 / F-CR-05 residual).
        NostrContentView(
            tree: tree,
            font: font,
            mentionLabel: { uri in renderContext.mentionLabel(for: uri.primaryId) }
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
