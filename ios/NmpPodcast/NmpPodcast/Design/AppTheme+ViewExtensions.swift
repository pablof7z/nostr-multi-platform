import SwiftUI

// MARK: - View extensions

extension View {
    /// Applies the canonical settings-screen List presentation: inset-grouped style,
    /// hidden scroll content background, and a `systemGroupedBackground` fill.
    func settingsListStyle() -> some View {
        self
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(Color(.systemGroupedBackground).ignoresSafeArea())
    }

    /// Constrains text to a single line and truncates in the middle — the
    /// canonical treatment for IDs, pubkeys, and other opaque strings where
    /// both the leading and trailing characters are meaningful.
    func truncatedMiddle() -> some View {
        self
            .lineLimit(1)
            .truncationMode(.middle)
    }

    /// Standard secondary-background card with a rounded rectangle clip.
    func cardSurface(cornerRadius: CGFloat = AppTheme.Corner.md) -> some View {
        self.background(
            Color(.secondarySystemBackground),
            in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
        )
    }
}
