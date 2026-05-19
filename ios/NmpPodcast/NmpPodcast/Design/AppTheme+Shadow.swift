import SwiftUI

extension AppTheme {

    // MARK: - Shadows

    enum Shadow {
        struct Style {
            var color: SwiftUI.Color
            var radius: CGFloat
            var x: CGFloat
            var y: CGFloat
        }
        static let subtle = Style(color: .black.opacity(0.07), radius: 4, x: 0, y: 2)
        static let card = Style(color: .black.opacity(0.10), radius: 12, x: 0, y: 4)
        static let lifted = Style(color: .black.opacity(0.16), radius: 20, x: 0, y: 8)
        static let onboardingIconGlow = Style(color: .white.opacity(0.5), radius: 12, x: 0, y: 0)
    }
}

// MARK: - View extension

extension View {
    func appShadow(_ style: AppTheme.Shadow.Style) -> some View {
        self.shadow(color: style.color, radius: style.radius, x: style.x, y: style.y)
    }
}
