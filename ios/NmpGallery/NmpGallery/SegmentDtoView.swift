import SwiftUI

/// Shared `SegmentDto` walker → SwiftUI. Renders one `ContentTreeDto`
/// (the primary note body or any resolved embed body) by dispatching per
/// `Segment` variant, exactly as the Rust doctrine says every platform
/// renderer must (content-rendering.md §5). Embeds resolve against the
/// scenario's relay-free `embeds` map; the PD-015 depth + cycle guard is
/// applied here via the Swift `RenderContext` mirror.
struct SegmentDtoView: View {
    let tree: ContentTreeDto
    let embeds: [String: EmbedEntry]
    var ctx = RenderContext()

    @Environment(\.nmpMediaRenderer) private var media

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if tree.segments.isEmpty {
                Text("(empty content)")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .italic()
            }
            ForEach(Array(segmentGroups.enumerated()), id: \.offset) { _, group in
                switch group {
                case .inline(let segs):
                    inlineRun(segs)
                case .block(let seg):
                    blockView(seg)
                }
            }
        }
    }

    // MARK: - Segment grouping

    private enum SegmentGroup {
        case inline([SegmentDto])
        case block(SegmentDto)
    }

    private var segmentGroups: [SegmentGroup] {
        var groups: [SegmentGroup] = []
        var run: [SegmentDto] = []

        for seg in tree.segments {
            if seg.isInline {
                run.append(seg)
            } else {
                if !run.isEmpty { groups.append(.inline(run)); run = [] }
                groups.append(.block(seg))
            }
        }
        if !run.isEmpty { groups.append(.inline(run)) }
        return groups
    }

    // MARK: - Inline run

    /// Consecutive inline segments flow together. For runs containing only
    /// text/hashtag/url/invoice/mention, Text concatenation is used — this
    /// gives perfect word wrapping. Runs that include resolved custom emoji
    /// images fall back to FlowLayout so the AsyncImage can appear inline.
    @ViewBuilder
    private func inlineRun(_ segs: [SegmentDto]) -> some View {
        if segs.hasResolvedEmoji {
            FlowLayout(horizontalSpacing: 2, verticalSpacing: 4) {
                ForEach(Array(segs.enumerated()), id: \.offset) { _, seg in
                    flowItem(seg)
                }
            }
        } else {
            segs.reduce(Text("")) { acc, seg in acc + inlineText(seg) }
                .font(.body)
        }
    }

    /// Single view for one segment inside a FlowLayout row.
    @ViewBuilder
    private func flowItem(_ seg: SegmentDto) -> some View {
        switch seg {
        case let .text(t):
            Text(t).font(.body)
        case let .hashtag(tag):
            Text("#\(tag)").foregroundStyle(Color.accentColor).bold().font(.body)
        case let .url(u):
            Text(u).foregroundStyle(Color.blue).font(.body)
        case let .invoice(_, value):
            Text("⚡ \(value.prefix(12))…").foregroundStyle(Color.orange).font(.body)
        case let .mention(_, _, pubkey):
            Text("@npub1\(pubkey.prefix(6))…").foregroundStyle(Color.indigo).bold().font(.body)
        case let .emoji(shortcode, urlString):
            if let urlString, let url = URL(string: urlString) {
                AsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let img):
                        img.resizable().scaledToFit()
                    case .failure:
                        Text(":\(shortcode):").font(.body)
                    default:
                        // Placeholder sized so the row height is stable while loading.
                        Color.clear
                    }
                }
                .frame(width: 20, height: 20)
            } else {
                Text(":\(shortcode):").font(.body)
            }
        default:
            EmptyView()
        }
    }

    /// Text-safe (no image) representation of a segment for concatenation.
    private func inlineText(_ seg: SegmentDto) -> Text {
        switch seg {
        case let .text(t):
            return Text(t)
        case let .hashtag(tag):
            return Text("#\(tag)").foregroundStyle(Color.accentColor).bold()
        case let .url(u):
            return Text(u).foregroundStyle(Color.blue)
        case let .emoji(shortcode, _):
            return Text(":\(shortcode):")
        case let .invoice(_, value):
            return Text("⚡ \(value.prefix(12))…").foregroundStyle(Color.orange)
        case let .mention(_, _, pubkey):
            return Text("@npub1\(pubkey.prefix(6))…").foregroundStyle(Color.indigo).bold()
        default:
            return Text("")
        }
    }

    // MARK: - Block segments

    @ViewBuilder
    private func blockView(_ seg: SegmentDto) -> some View {
        switch seg {
        case let .media(kind, urls):
            mediaBlock(kind: kind, urls: urls)
        case let .eventRef(uri, _, id):
            EmbedCard(uri: uri, refID: id,
                      entry: embeds[uri], embeds: embeds, ctx: ctx)
        case let .markdownBlock(node):
            MarkdownNodeView(node: node, embeds: embeds, ctx: ctx)
        case let .unknown(type):
            Text("[unknown segment: \(type)]")
                .font(.caption).foregroundStyle(.red)
        default:
            EmptyView()
        }
    }

    // MARK: - Media block

    @ViewBuilder
    private func mediaBlock(kind: String, urls: [String]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(urls, id: \.self) { urlString in
                if let url = URL(string: urlString) {
                    switch kind {
                    case "Image":
                        media.imageView(url)
                    case "Video":
                        media.videoView(url)
                    default:
                        Label(urlString, systemImage: icon(for: kind))
                            .font(.caption)
                            .foregroundStyle(.purple)
                            .lineLimit(1)
                    }
                } else {
                    Label(urlString, systemImage: "exclamationmark.triangle")
                        .font(.caption2.monospaced())
                        .foregroundStyle(.red)
                        .lineLimit(1)
                }
            }
        }
    }

    // MARK: - Helpers

    @ViewBuilder
    private func chip(_ text: String, system: String, tint: Color) -> some View {
        Label(text, systemImage: system)
            .font(.caption.bold())
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(tint.opacity(0.18))
            .foregroundStyle(tint)
            .clipShape(Capsule())
    }

    private func icon(for kind: String) -> String {
        switch kind {
        case "Image": return "photo"
        case "Video": return "play.rectangle"
        case "Audio": return "waveform"
        default: return "doc"
        }
    }
}

// MARK: - SegmentDto inline classification

private extension SegmentDto {
    /// True for segments that can participate in a wrapping inline run.
    var isInline: Bool {
        switch self {
        case .text, .hashtag, .url, .emoji, .invoice, .mention:
            return true
        case .media, .eventRef, .markdownBlock, .unknown:
            return false
        }
    }
}

private extension [SegmentDto] {
    /// True when any element is a custom emoji with a resolved image URL.
    var hasResolvedEmoji: Bool {
        contains { seg in
            if case let .emoji(_, url) = seg { return url != nil }
            return false
        }
    }
}

// MARK: - MentionChip

/// Profile mention chip. Resolves kind:0 from the relay-free store;
/// falls back to a D1 deterministic identicon + truncated npub when the
/// profile is absent or has no picture.
struct MentionChip: View {
    let uri: String
    let pubkey: String
    let entry: EmbedEntry?

    var body: some View {
        HStack(spacing: 6) {
            Identicon(seed: pubkey)
                .frame(width: 22, height: 22)
            VStack(alignment: .leading, spacing: 0) {
                Text(displayName)
                    .font(.caption.bold())
                if entry?.profileName == nil {
                    Text("npub1\(pubkey.prefix(8))…")
                        .font(.caption2.monospaced())
                        .foregroundStyle(.secondary)
                }
            }
            if entry?.profilePicture != nil {
                Image(systemName: "photo.circle")
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(Color.indigo.opacity(0.14))
        .clipShape(Capsule())
    }

    private var displayName: String {
        if let n = entry?.profileName { return "@\(n)" }
        return "@npub1\(pubkey.prefix(6))…"
    }
}

// MARK: - Identicon

/// Deterministic identicon (D1 placeholder) — a stable 5×5 symmetric
/// blocky avatar seeded by the pubkey hex, never blank.
struct Identicon: View {
    let seed: String

    var body: some View {
        let bytes = Array(seed.utf8)
        let hue = Double(bytes.reduce(0) { Int($0) &+ Int($1) } % 360)
        GeometryReader { geo in
            let cell = geo.size.width / 5
            ForEach(0..<5, id: \.self) { row in
                ForEach(0..<3, id: \.self) { col in
                    let on = bit(bytes, row * 3 + col)
                    if on {
                        Rectangle()
                            .fill(Color(hue: hue / 360,
                                        saturation: 0.55,
                                        brightness: 0.75))
                            .frame(width: cell, height: cell)
                            .position(
                                x: cell * (Double(col) + 0.5),
                                y: cell * (Double(row) + 0.5))
                        Rectangle()
                            .fill(Color(hue: hue / 360,
                                        saturation: 0.55,
                                        brightness: 0.75))
                            .frame(width: cell, height: cell)
                            .position(
                                x: cell * (Double(4 - col) + 0.5),
                                y: cell * (Double(row) + 0.5))
                    }
                }
            }
        }
        .background(Color.gray.opacity(0.15))
        .clipShape(RoundedRectangle(cornerRadius: 4))
    }

    private func bit(_ bytes: [UInt8], _ i: Int) -> Bool {
        guard !bytes.isEmpty else { return false }
        return bytes[i % bytes.count] & 1 == 1
    }
}
