import SwiftUI

/// Reusable iOS-style settings row for use inside a `List`.
///
/// - Parameters:
///   - icon: SF Symbol name
///   - tint: Fill color of the `iconBadgeSize` × `iconBadgeSize` rounded icon badge
///   - title: Primary label
///   - subtitle: Optional secondary label shown below title
///   - value: Optional trailing value text
///   - badge: When > 0, shows an orange `StatBadge` trailing
struct SettingsRow: View {

    // MARK: - Layout constants

    private enum Layout {
        /// Side length of the icon badge square.
        static let iconBadgeSize: CGFloat = 29
        /// Corner radius of the icon badge — matches the iOS Settings app.
        static let iconBadgeCornerRadius: CGFloat = 7
        /// Point size of the SF Symbol inside the icon badge.
        static let iconFontSize: CGFloat = 15
        /// Horizontal gap between icon badge and label stack.
        static let rowSpacing: CGFloat = 12
        /// Sub-label spacing inside `labelStack`.
        static let labelSpacing: CGFloat = 2
        /// Minimum gap between label stack and trailing content.
        static let spacerMinLength: CGFloat = 4
    }

    /// Leading offset to the start of the label column — icon badge + row spacing.
    ///
    /// Sibling views that need to visually align their content with `SettingsRow`
    /// labels (e.g. accessory cards below a row) should use this constant rather
    /// than hardcoding the magic sum `29 + 12 = 41`.
    static let contentLeadingInset: CGFloat = Layout.iconBadgeSize + Layout.rowSpacing

    let icon: String
    let tint: Color
    let title: String
    var subtitle: String? = nil
    var value: String? = nil
    var badge: Int = 0

    var body: some View {
        HStack(spacing: Layout.rowSpacing) {
            iconBadge

            labelStack

            Spacer(minLength: Layout.spacerMinLength)

            trailingContent
        }
    }

    // MARK: - Sub-views

    private var iconBadge: some View {
        ZStack {
            RoundedRectangle(cornerRadius: Layout.iconBadgeCornerRadius, style: .continuous)
                .fill(tint)
                .frame(width: Layout.iconBadgeSize, height: Layout.iconBadgeSize)
            Image(systemName: icon)
                .font(.system(size: Layout.iconFontSize, weight: .semibold))
                .foregroundStyle(.white)
        }
        .accessibilityHidden(true)
    }

    @ViewBuilder
    private var labelStack: some View {
        if let subtitle {
            VStack(alignment: .leading, spacing: Layout.labelSpacing) {
                Text(title)
                    .font(AppTheme.Typography.body)
                Text(subtitle)
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.secondary)
                    .truncatedMiddle()
            }
        } else {
            Text(title)
                .font(AppTheme.Typography.body)
        }
    }

    @ViewBuilder
    private var trailingContent: some View {
        if badge > 0 {
            StatBadge(value: badge, label: nil, color: .orange)
        } else if let value {
            Text(value)
                .font(AppTheme.Typography.body)
                .foregroundStyle(.secondary)
                .truncatedMiddle()
        }
    }
}
