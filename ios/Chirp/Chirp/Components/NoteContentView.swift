import SwiftUI
struct NoteContentView: View {
    let content: String
    let contentTree: ContentTreeWire?
    let mentionProfiles: [String: MentionProfile]
    var font: Font = .body

    init(
        content: String,
        contentTree: ContentTreeWire? = nil,
        mentionProfiles: [String: MentionProfile] = [:],
        font: Font = .body
    ) {
        self.content = content
        self.contentTree = contentTree
        self.mentionProfiles = mentionProfiles
        self.font = font
    }

    var body: some View {
        if let contentTree {
            richBody(contentTree)
        } else {
            legacyBody
        }
    }

    @ViewBuilder
    private func richBody(_ tree: ContentTreeWire) -> some View {
        let groups = richGroups(tree)
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

    private var legacyBody: some View {
        let groups = tokenGroups(NoteToken.tokenize(content))
        return VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(groups.enumerated()), id: \.offset) { _, group in
                switch group {
                case .inline(let toks):
                    toks.reduce(Text("")) { acc, t in acc + t.inlineText() }
                        .font(font)
                case .image(let url):
                    imageView(url)
                case .video(let url):
                    videoPlaceholder(url)
                }
            }
        }
    }
    private enum RichGroup {
        case inline([UInt32])
        case media(urls: [String], kind: String)
        case eventRef(WireNostrUri)
    }

    private func richGroups(_ tree: ContentTreeWire) -> [RichGroup] {
        var groups: [RichGroup] = []
        var run: [UInt32] = []

        func flush() {
            if !run.isEmpty {
                groups.append(.inline(run))
                run = []
            }
        }

        for root in tree.roots {
            guard let node = node(root, in: tree) else { continue }
            switch node {
            case .media(let urls, let kind):
                flush()
                groups.append(.media(urls: urls, kind: kind))
            case .eventRef(let uri):
                flush()
                groups.append(.eventRef(uri))
            case .paragraph(let children), .heading(_, let children):
                run.append(contentsOf: children)
                run.append(UInt32.max)
            default:
                run.append(root)
            }
        }
        flush()
        return groups
    }

    private func inlineText(_ index: UInt32, in tree: ContentTreeWire) -> Text {
        if index == UInt32.max { return Text("\n") }
        guard let n = node(index, in: tree) else { return Text("") }
        switch n {
        case .text(let value):
            return Text(value)
        case .mention(let uri):
            let label = mentionProfiles[uri.primaryId]?.display ?? shortHex(uri.primaryId)
            return Text("@\(label)").foregroundStyle(Color.accentColor).bold()
        case .eventRef(let uri):
            return Text("↩ \(shortHex(uri.primaryId))").foregroundStyle(Color.accentColor).bold()
        case .hashtag(let tag):
            return Text("#\(tag)").foregroundStyle(Color.accentColor).bold()
        case .url(let value):
            return Text(value).foregroundStyle(Color.accentColor)
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
        case .media, .placeholder:
            return Text("")
        }
    }

    @ViewBuilder
    private func mediaView(urls: [String], kind: String) -> some View {
        if kind == "Image", let first = urls.first.flatMap(URL.init(string:)) {
            imageView(first)
        } else if let first = urls.first.flatMap(URL.init(string:)) {
            videoPlaceholder(first)
        }
    }

    private func eventReferenceView(_ uri: WireNostrUri) -> some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: "quote.bubble")
                .font(.callout.weight(.semibold))
                .foregroundStyle(Color.accentColor)
            VStack(alignment: .leading, spacing: 2) {
                Text("Referenced event")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.primary)
                Text(shortHex(uri.primaryId))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .padding(10)
        .background(Color.secondary.opacity(0.10), in: RoundedRectangle(cornerRadius: 8))
    }

    private func imageView(_ url: URL) -> some View {
        AsyncImage(url: url) { phase in
            switch phase {
            case .success(let img):
                img.resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: 300)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
            case .empty:
                RoundedRectangle(cornerRadius: 10)
                    .fill(Color.secondary.opacity(0.12))
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

    private func shortHex(_ value: String) -> String {
        guard value.count > 12 else { return value }
        return "\(value.prefix(8))…\(value.suffix(4))"
    }
    private enum TokenGroup {
        case inline([NoteToken])
        case image(URL)
        case video(URL)
    }

    private func tokenGroups(_ tokens: [NoteToken]) -> [TokenGroup] {
        var groups: [TokenGroup] = []
        var run: [NoteToken] = []

        func flush() {
            if !run.isEmpty {
                let allWhitespace = run.allSatisfy {
                    if case .text(let s) = $0 { return s.allSatisfy(\.isWhitespace) }
                    return false
                }
                if !allWhitespace { groups.append(.inline(run)) }
                run = []
            }
        }

        for token in tokens {
            switch token {
            case .image(let url): flush(); groups.append(.image(url))
            case .video(let url): flush(); groups.append(.video(url))
            default: run.append(token)
            }
        }
        flush()
        return groups
    }
}

enum NoteToken {
    case text(String)
    case hashtag(String)
    case url(String)
    case mention(String)
    case image(URL)
    case video(URL)

    func inlineText() -> Text {
        switch self {
        case .text(let s): return Text(s)
        case .hashtag(let tag): return Text("#\(tag)").foregroundStyle(Color.accentColor).bold()
        case .url(let u): return Text(u).foregroundStyle(Color.accentColor)
        case .mention(let bech32):
            return Text("@\(bech32.prefix(10))…").foregroundStyle(Color.accentColor).bold()
        case .image, .video: return Text("")
        }
    }

    static func tokenize(_ content: String) -> [NoteToken] {
        let pattern = /nostr:[a-z0-9]+|https?:\/\/\S+|#[a-zA-Z]\w*/
        var tokens: [NoteToken] = []
        var lastEnd = content.startIndex

        for match in content.matches(of: pattern) {
            if match.range.lowerBound > lastEnd {
                tokens.append(.text(String(content[lastEnd..<match.range.lowerBound])))
            }
            let raw = String(match.output)
            if raw.hasPrefix("nostr:") {
                tokens.append(.mention(String(raw.dropFirst(6))))
            } else if raw.hasPrefix("#") {
                tokens.append(.hashtag(String(raw.dropFirst())))
            } else if let url = URL(string: raw), url.scheme?.hasPrefix("http") == true {
                let ext = url.pathExtension.lowercased()
                if imageExtensions.contains(ext) {
                    tokens.append(.image(url))
                } else if videoExtensions.contains(ext) {
                    tokens.append(.video(url))
                } else {
                    tokens.append(.url(raw))
                }
            } else {
                tokens.append(.text(raw))
            }
            lastEnd = match.range.upperBound
        }

        if lastEnd < content.endIndex {
            tokens.append(.text(String(content[lastEnd...])))
        }
        return tokens
    }

    private static let imageExtensions: Set<String> = ["jpg", "jpeg", "png", "gif", "webp", "avif", "svg", "heic"]
    private static let videoExtensions: Set<String> = ["mp4", "mov", "webm", "m4v", "mkv"]
}
