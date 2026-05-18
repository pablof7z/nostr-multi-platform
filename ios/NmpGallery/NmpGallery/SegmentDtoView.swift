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

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if tree.segments.isEmpty {
                Text("(empty content)")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .italic()
            }
            ForEach(Array(tree.segments.enumerated()), id: \.offset) {
                _, seg in
                segmentView(seg)
            }
        }
    }

    @ViewBuilder
    private func segmentView(_ seg: SegmentDto) -> some View {
        switch seg {
        case let .text(t):
            Text(t)
                .font(.body)
        case let .hashtag(tag):
            chip("#\(tag)", system: "number", tint: .blue)
        case let .url(u):
            Label(u, systemImage: "link")
                .font(.callout)
                .foregroundStyle(.blue)
                .lineLimit(1)
        case let .media(kind, urls):
            mediaTile(kind: kind, urls: urls)
        case let .emoji(shortcode, url):
            chip(
                ":\(shortcode):" + (url == nil ? " (unresolved)" : ""),
                system: url == nil ? "questionmark.circle" : "face.smiling",
                tint: url == nil ? .orange : .pink)
        case let .invoice(kind, value):
            chip("\(kind) invoice · \(value.prefix(14))…",
                 system: "bolt.fill", tint: .yellow)
        case let .mention(uri, _, pubkey):
            MentionChip(uri: uri, pubkey: pubkey,
                        entry: embeds[uri])
        case let .eventRef(uri, _, id):
            EmbedCard(uri: uri, refID: id,
                      entry: embeds[uri], embeds: embeds, ctx: ctx)
        case let .markdownBlock(node):
            MarkdownNodeView(node: node, embeds: embeds, ctx: ctx)
        case let .unknown(type):
            Text("[unknown segment: \(type)]")
                .font(.caption).foregroundStyle(.red)
        }
    }

    @ViewBuilder
    private func chip(_ text: String, system: String,
                      tint: Color) -> some View {
        Label(text, systemImage: system)
            .font(.caption.bold())
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(tint.opacity(0.18))
            .foregroundStyle(tint)
            .clipShape(Capsule())
    }

    @ViewBuilder
    private func mediaTile(kind: String, urls: [String]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Label("\(kind) · \(urls.count) item(s)",
                  systemImage: icon(for: kind))
                .font(.caption.bold())
                .foregroundStyle(.purple)
            ForEach(urls, id: \.self) { u in
                Text(u).font(.caption2.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.purple.opacity(0.10))
        .clipShape(RoundedRectangle(cornerRadius: 8))
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
