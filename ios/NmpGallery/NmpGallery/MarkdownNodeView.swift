import SwiftUI

/// Renders one `MarkdownNodeDto` (CommonMark-core only, PD-012: no
/// tables, no strikethrough — those arrive as literal text per the
/// verified `Options::empty()` behaviour). Inline runs reuse the same
/// `Segment` shape as plaintext, so `nostr:` mentions inside an article
/// body resolve through the same embed store.
struct MarkdownNodeView: View {
    let node: MarkdownNodeDto
    let embeds: [String: EmbedEntry]
    let ctx: RenderContext

    var body: some View {
        switch node {
        case let .heading(level, inlines):
            inlineRow(inlines)
                .font(headingFont(level))
                .bold()
        case let .paragraph(inlines):
            inlineRow(inlines)
                .font(.body)
        case let .blockQuote(blocks):
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(blocks.enumerated()), id: \.offset) {
                    _, b in
                    MarkdownNodeView(node: b, embeds: embeds, ctx: ctx)
                }
            }
            .padding(.leading, 10)
            .overlay(alignment: .leading) {
                Rectangle().frame(width: 3)
                    .foregroundStyle(.secondary)
            }
        case let .codeBlock(info, body):
            VStack(alignment: .leading, spacing: 2) {
                if let info { Text(info).font(.caption2)
                    .foregroundStyle(.secondary) }
                Text(body)
                    .font(.caption.monospaced())
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
                    .background(Color(.secondarySystemBackground))
                    .clipShape(RoundedRectangle(cornerRadius: 6))
            }
        case let .list(orderedStart, items):
            VStack(alignment: .leading, spacing: 3) {
                ForEach(Array(items.enumerated()), id: \.offset) {
                    idx, item in
                    HStack(alignment: .top, spacing: 6) {
                        Text(marker(orderedStart, idx))
                            .font(.body.monospaced())
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(Array(item.enumerated()),
                                    id: \.offset) { _, b in
                                MarkdownNodeView(
                                    node: b, embeds: embeds, ctx: ctx)
                            }
                        }
                    }
                }
            }
        case .rule:
            Divider().padding(.vertical, 2)
        case let .unknown(t):
            Text("[md: \(t)]").font(.caption).foregroundStyle(.red)
        }
    }

    private func marker(_ start: UInt64?, _ idx: Int) -> String {
        if let start { return "\(start + UInt64(idx))." }
        return "•"
    }

    private func headingFont(_ level: Int) -> Font {
        switch level {
        case 1: return .title
        case 2: return .title2
        case 3: return .title3
        default: return .headline
        }
    }

    @ViewBuilder
    private func inlineRow(_ inlines: [MarkdownInlineDto]) -> some View {
        InlineFlow(inlines: inlines, embeds: embeds, ctx: ctx)
    }
}

/// Flattens markdown inline runs to a wrapping text/segment row. Emphasis
/// / strong / code map to text styling; `Inline(Segment)` delegates to
/// the shared `SegmentDtoView` so mentions + event refs inside article
/// bodies resolve identically to plaintext.
struct InlineFlow: View {
    let inlines: [MarkdownInlineDto]
    let embeds: [String: EmbedEntry]
    let ctx: RenderContext

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            ForEach(Array(inlines.enumerated()), id: \.offset) { _, i in
                inlineView(i)
            }
        }
    }

    @ViewBuilder
    private func inlineView(_ i: MarkdownInlineDto) -> some View {
        switch i {
        case let .inline(seg):
            SegmentDtoView(
                tree: ContentTreeDto(mode: "Markdown", segments: [seg]),
                embeds: embeds, ctx: ctx)
        case let .emphasis(children):
            InlineFlow(inlines: children, embeds: embeds, ctx: ctx)
                .italic()
        case let .strong(children):
            InlineFlow(inlines: children, embeds: embeds, ctx: ctx)
                .bold()
        case let .code(text):
            Text(text)
                .font(.callout.monospaced())
                .padding(.horizontal, 4)
                .background(Color(.secondarySystemBackground))
                .clipShape(RoundedRectangle(cornerRadius: 4))
        case let .link(label, href):
            HStack(spacing: 2) {
                Image(systemName: "link").font(.caption2)
                InlineFlow(inlines: label, embeds: embeds, ctx: ctx)
            }
            .foregroundStyle(.blue)
            .help(href ?? "")
        case let .image(alt, _, src):
            Label("\(alt) [\(src ?? "no src")]",
                  systemImage: "photo")
                .font(.caption)
                .foregroundStyle(.purple)
        case .softBreak, .hardBreak:
            EmptyView()
        case let .unknown(t):
            Text("[inline: \(t)]").font(.caption2)
                .foregroundStyle(.red)
        }
    }
}

/// Top-level dispatcher for a scenario's primary rendered tree.
struct ScenarioRenderer: View {
    let scenario: Scenario

    var body: some View {
        SegmentDtoView(
            tree: scenario.rendered,
            embeds: scenario.embeds,
            ctx: RenderContext())
    }
}
