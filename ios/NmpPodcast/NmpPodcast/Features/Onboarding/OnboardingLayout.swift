import CoreFoundation

// MARK: - Shared layout constants

/// Shared layout constants referenced by multiple onboarding page views.
enum OnboardingLayout {
    static let pageIconSize: CGFloat = 60
    static let pageIconPadding: CGFloat = 28
    static let pageIconStroke: CGFloat = 0.3
    static let fieldVerticalPadding: CGFloat = 14
    /// Corner radius for the welcome sparkle medallion and its glass overlay.
    static let medallionCornerRadius: CGFloat = 36
    /// Side length of the welcome sparkle medallion tile.
    static let medallionSize: CGFloat = 148
    /// Point size of the sparkle SF Symbol inside the welcome medallion.
    static let medallionIconSize: CGFloat = 76
    /// Minimum height of the primary action button on each page (touch target).
    static let primaryButtonMinHeight: CGFloat = 28
    /// Vertical padding inside the primary action button.
    static let primaryButtonVerticalPadding: CGFloat = 8
}
