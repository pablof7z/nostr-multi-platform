import SwiftUI

/// Small glass capsule badge for counts and short labels.
/// Use inside GlassEffectContainer when placing alongside other glass views.
struct StatBadge: View {
    let value: Int
    var label: String? = nil
    var color: Color = .accentColor

    // MARK: - Layout constants

    private enum Layout {
        /// Horizontal spacing between the value and the optional label text.
        static let innerSpacing: CGFloat = 2
        /// Horizontal padding when the badge shows a single digit with no label.
        static let paddingHNarrow: CGFloat = 6
        /// Horizontal padding when the badge shows a multi-digit value or a label.
        static let paddingHWide: CGFloat = 8
        /// Vertical padding inside the capsule.
        static let paddingV: CGFloat = 3
        /// Threshold below which the single-digit narrow padding is used.
        static let singleDigitThreshold = 10
    }

    var body: some View {
        HStack(spacing: Layout.innerSpacing) {
            Text("\(value)")
                .font(AppTheme.Typography.caption.weight(.bold).monospacedDigit())
            if let label {
                Text(label)
                    .font(AppTheme.Typography.caption2.weight(.medium))
            }
        }
        .padding(.horizontal, (value < Layout.singleDigitThreshold && label == nil)
            ? Layout.paddingHNarrow
            : Layout.paddingHWide)
        .padding(.vertical, Layout.paddingV)
        .foregroundStyle(color)
        .glassEffect(.regular.tint(color), in: .capsule)
    }
}

// MARK: - Convenience factory methods

extension StatBadge {
    static func count(_ count: Int, color: Color = .secondary) -> StatBadge {
        StatBadge(value: count, color: color)
    }
}
