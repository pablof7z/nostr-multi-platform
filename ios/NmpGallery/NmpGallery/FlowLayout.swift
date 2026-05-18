import SwiftUI

/// Wrapping row layout (left-to-right, wraps like text). Used for inline
/// runs that contain non-Text elements such as resolved custom emoji images.
struct FlowLayout: Layout {
    var horizontalSpacing: CGFloat = 2
    var verticalSpacing: CGFloat = 4

    func makeCache(subviews: Subviews) -> LayoutCache {
        LayoutCache()
    }

    func sizeThatFits(
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout LayoutCache
    ) -> CGSize {
        cache = solve(proposal: proposal, subviews: subviews)
        return cache.size
    }

    func placeSubviews(
        in bounds: CGRect,
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout LayoutCache
    ) {
        if cache.frames.isEmpty { cache = solve(proposal: proposal, subviews: subviews) }
        for (i, frame) in cache.frames.enumerated() {
            subviews[i].place(
                at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                proposal: ProposedViewSize(frame.size))
        }
    }

    // MARK: - Cache

    struct LayoutCache {
        var frames: [CGRect] = []
        var size: CGSize = .zero
    }

    // MARK: - Solver

    private func solve(proposal: ProposedViewSize, subviews: Subviews) -> LayoutCache {
        let maxWidth = proposal.replacingUnspecifiedDimensions().width
        var frames: [CGRect] = []
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0

        for subview in subviews {
            let ideal = subview.sizeThatFits(.unspecified)
            // If item doesn't fit on the current row (and isn't the first on a row), wrap.
            if x > 0, x + ideal.width > maxWidth {
                x = 0
                y += rowHeight + verticalSpacing
                rowHeight = 0
            }
            // Re-size with a width constraint so multiline Text wraps instead of clipping.
            let available = maxWidth - x
            let size = subview.sizeThatFits(
                ProposedViewSize(width: min(ideal.width, available), height: nil))
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            x += size.width + horizontalSpacing
            rowHeight = max(rowHeight, size.height)
        }

        return LayoutCache(
            frames: frames,
            size: CGSize(width: maxWidth, height: y + rowHeight))
    }
}
