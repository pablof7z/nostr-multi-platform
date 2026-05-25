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
            timelineItems: timelineItems,
            embedDepth: 0
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
        let groups = noteContentGroups(tree)
        if groups.isEmpty {
            EmptyView()
        } else {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(groups.enumerated()), id: \.offset) { _, group in
                    switch group {
                    case .inline(let nodes):
                        nodes.reduce(Text("")) { acc, node in
                            acc + inlineText(node, in: tree)
                        }
                        .font(font)
                    case .media(let urls, let kind):
                        mediaView(urls: urls, kind: kind)
                    case .eventRef(let uri):
                        eventReferenceView(uri)
                    }
                }
            }
        }
    }

    private var plainBody: some View {
        Text(content)
            .font(font)
    }

    private func inlineText(_ index: UInt32, in tree: ContentTreeWire) -> Text {
        if index == UInt32.max { return Text("\n") }
        guard let n = node(index, in: tree) else { return Text("") }
        switch n {
        case .text(let value):
            return Text(value)
        case .mention(let uri):
            let label = renderContext.mentionLabel(for: uri.primaryId)
            return Text("@\(label)").foregroundStyle(ChirpColor.link).bold()
        case .eventRef(let uri):
            return Text("↩ \(shortEntity(uri.primaryId))").foregroundStyle(ChirpColor.link).bold()
        case .hashtag(let tag):
            return Text("#\(tag)").foregroundStyle(ChirpColor.link).bold()
        case .url(let value):
            return Text(value).foregroundStyle(ChirpColor.link)
        case .emoji(let shortcode, _):
            return Text(":\(shortcode):")
        case .emphasis(let children):
            return children.reduce(Text("")) { $0 + inlineText($1, in: tree).italic() }
        case .strong(let children):
            return children.reduce(Text("")) { $0 + inlineText($1, in: tree).bold() }
        case .inlineCode(let value):
            return Text(value).font(.system(.body, design: .monospaced))
        case .softBreak:
            return Text(" ")
        case .hardBreak:
            return Text("\n")
        case .paragraph(let children), .heading(_, let children):
            return children.reduce(Text("")) { $0 + inlineText($1, in: tree) }
        case .blockQuote(let children):
            return children.reduce(Text("")) { $0 + inlineText($1, in: tree) }
        case .link(let children, _):
            return children.reduce(Text("")) { $0 + inlineText($1, in: tree) }
        case .invoice:
            return Text("⚡ invoice").foregroundStyle(ChirpColor.link)
        case .image(let alt, _, _):
            return Text(alt.isEmpty ? "[image]" : "[\(alt)]").foregroundStyle(.secondary)
        case .media, .codeBlock, .list, .rule, .placeholder(_):
            return Text("")
        }
    }

    @ViewBuilder
    private func mediaView(urls: [String], kind: MediaKind) -> some View {
        switch kind {
        case .image:
            let imageURLs = urls.compactMap(URL.init(string:))
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(imageURLs.enumerated()), id: \.offset) { _, url in
                    imageView(url)
                }
            }
        case .video, .audio:
            // Audio and video share the same compact media row until the
            // content-tree media renderer grows separate audio controls.
            if let first = urls.first.flatMap(URL.init(string:)) {
                videoPlaceholder(first)
            }
        }
    }

    private func eventReferenceView(_ uri: WireNostrUri) -> some View {
        EmbeddedNostrEventCard(uri: uri, context: renderContext)
    }

    private func imageView(_ url: URL) -> some View {
        AsyncImage(url: url) { phase in
            switch phase {
            case .success(let img):
                Button {
                    tappedImage = TappedImage(url: url)
                } label: {
                    img.resizable()
                        .scaledToFit()
                        .frame(maxWidth: .infinity, maxHeight: 300)
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                        .fadeIn()
                }
                .buttonStyle(.plain)
            case .empty:
                RoundedRectangle(cornerRadius: 10)
                    .fill(ChirpColor.secondaryFill)
                    .frame(maxWidth: .infinity, minHeight: 80, maxHeight: 120)
            default:
                EmptyView()
            }
        }
    }

    private func videoPlaceholder(_ url: URL) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "play.rectangle.fill")
                .font(.title2)
                .foregroundStyle(.primary)
            Text(url.lastPathComponent)
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Spacer()
        }
        .padding(12)
        .frame(maxWidth: .infinity)
        .overlay(alignment: .bottom) { Divider() }
    }

    private func node(_ index: UInt32, in tree: ContentTreeWire) -> ContentWireNode? {
        let i = Int(index)
        guard i >= 0, i < tree.nodes.count else { return nil }
        return tree.nodes[i]
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
