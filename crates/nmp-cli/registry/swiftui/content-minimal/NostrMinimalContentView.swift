import SwiftUI

public struct NostrContentRun: Identifiable, Equatable {
    public enum Kind: Equatable {
        case text
        case mention(pubkey: String)
        case hashtag(String)
        case link(URL)
    }

    public var id: String
    public var label: String
    public var kind: Kind

    public init(id: String, label: String, kind: Kind) {
        self.id = id
        self.label = label
        self.kind = kind
    }
}

public struct NostrMinimalContentView: View {
    public var runs: [NostrContentRun]
    @Environment(\.nostrContentRenderer) private var renderer

    public init(runs: [NostrContentRun]) {
        self.runs = runs
    }

    public var body: some View {
        FlowLayout(spacing: 4) {
            ForEach(runs) { run in
                runView(run)
            }
        }
    }

    @ViewBuilder
    private func runView(_ run: NostrContentRun) -> some View {
        switch run.kind {
        case .text:
            Text(run.label)
                .foregroundStyle(renderer.textColor)
        case .mention(let pubkey):
            Button(run.label) {
                renderer.callbacks.onMentionTap(pubkey)
            }
            .buttonStyle(.plain)
            .foregroundStyle(renderer.mentionColor)
        case .hashtag(let tag):
            Button(run.label) {
                renderer.callbacks.onHashtagTap(tag)
            }
            .buttonStyle(.plain)
            .foregroundStyle(renderer.hashtagColor)
        case .link(let url):
            Button(run.label) {
                renderer.callbacks.onLinkTap(url)
            }
            .buttonStyle(.plain)
            .foregroundStyle(renderer.linkColor)
        }
    }
}

private struct FlowLayout: Layout {
    var spacing: CGFloat

    func sizeThatFits(
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout ()
    ) -> CGSize {
        layout(in: proposal.replacingUnspecifiedDimensions().width, subviews: subviews).size
    }

    func placeSubviews(
        in bounds: CGRect,
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout ()
    ) {
        let rows = layout(in: bounds.width, subviews: subviews).rows
        for row in rows {
            for item in row.items {
                subviews[item.index].place(
                    at: CGPoint(x: bounds.minX + item.origin.x, y: bounds.minY + item.origin.y),
                    proposal: ProposedViewSize(item.size)
                )
            }
        }
    }

    private func layout(in width: CGFloat, subviews: Subviews) -> FlowResult {
        var rows: [FlowRow] = []
        var current = FlowRow()
        var cursor = CGPoint.zero
        var rowHeight: CGFloat = 0

        for index in subviews.indices {
            let size = subviews[index].sizeThatFits(.unspecified)
            if cursor.x > 0, cursor.x + size.width > width {
                rows.append(current)
                cursor = CGPoint(x: 0, y: cursor.y + rowHeight + spacing)
                rowHeight = 0
                current = FlowRow()
            }

            current.items.append(FlowItem(index: index, origin: cursor, size: size))
            cursor.x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }

        rows.append(current)
        let height = rows.last?.items.last.map { $0.origin.y + rowHeight } ?? 0
        return FlowResult(rows: rows, size: CGSize(width: width, height: height))
    }
}

private struct FlowResult {
    var rows: [FlowRow]
    var size: CGSize
}

private struct FlowRow {
    var items: [FlowItem] = []
}

private struct FlowItem {
    var index: Int
    var origin: CGPoint
    var size: CGSize
}
