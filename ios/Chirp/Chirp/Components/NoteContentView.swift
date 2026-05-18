import SwiftUI

// Swift-side note content renderer. Tokenizes the raw event content string
// without an FFI round-trip — suitable for a 60fps feed. Handles the most
// common segment types: text, hashtag, URL, image, video, nostr: mentions.
// Full resolution (embed cards, NIP-30 emoji images, invoices) lives in the
// kernel and will be wired when the ContentTreeDto projection is added.

struct NoteContentView: View {
    let content: String
    var font: Font = .body

    var body: some View {
        let groups = tokenGroups(NoteToken.tokenize(content))
        if groups.isEmpty { EmptyView() }
        else {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(groups.enumerated()), id: \.offset) { _, group in
                    switch group {
                    case .inline(let toks):
                        toks.reduce(Text("")) { acc, t in acc + t.inlineText() }
                            .font(font)
                    case .image(let url):
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
                    case .video(let url):
                        HStack(spacing: 10) {
                            Image(systemName: "play.rectangle.fill")
                                .font(.title2)
                                .foregroundStyle(.white)
                            Text(url.lastPathComponent)
                                .font(.caption.monospaced())
                                .foregroundStyle(.white.opacity(0.7))
                                .lineLimit(1)
                            Spacer()
                        }
                        .padding(12)
                        .frame(maxWidth: .infinity)
                        .background(Color.black.opacity(0.72))
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                    }
                }
            }
        }
    }

    // MARK: - Grouping

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
                // Drop inline groups that are nothing but whitespace.
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

// MARK: - NoteToken

enum NoteToken {
    case text(String)
    case hashtag(String)    // tag without leading #
    case url(String)        // plain link (non-media)
    case mention(String)    // bech32 entity following "nostr:"
    case image(URL)
    case video(URL)

    func inlineText() -> Text {
        switch self {
        case .text(let s):
            return Text(s)
        case .hashtag(let tag):
            return Text("#\(tag)").foregroundStyle(Color.accentColor).bold()
        case .url(let u):
            return Text(u).foregroundStyle(Color.blue)
        case .mention(let bech32):
            // Show first 10 chars of the bech32 key — no profile lookup needed.
            let prefix = bech32.prefix(10)
            let kind = bech32.hasPrefix("npub1") ? "@" : "↩ "
            return Text("\(kind)\(prefix)…").foregroundStyle(Color.indigo).bold()
        case .image, .video:
            return Text("")
        }
    }

    // MARK: Tokenizer

    static func tokenize(_ content: String) -> [NoteToken] {
        // Matches (in priority order):
        //   1. nostr:<bech32>   — mention or event ref
        //   2. https?://\S+     — URL (then classified as image/video/plain)
        //   3. #<word>          — hashtag (must start with letter, not a pure number)
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

    private static let imageExtensions: Set<String> = [
        "jpg", "jpeg", "png", "gif", "webp", "avif", "svg", "heic"
    ]
    private static let videoExtensions: Set<String> = [
        "mp4", "mov", "webm", "m4v", "mkv"
    ]
}
