import SwiftUI

/// Central design-token namespace for the app.
///
/// Use nested enums (`Spacing`, `Corner`, `Typography`, etc.) to access
/// individual tokens. Never hardcode raw values where a token exists.
///
/// Tokens are split into focused extension files:
///   - `AppTheme+Animation.swift` — Animation + Timing
///   - `AppTheme+Colors.swift`    — Brand + Tint + Gradients
///   - `AppTheme+Shadow.swift`    — Shadow + appShadow view extension
///   - `AppTheme+Typography.swift` — Typography
///   - `AppTheme+ViewExtensions.swift` — settingsListStyle / truncatedMiddle / cardSurface
enum AppTheme {

    // MARK: - Spacing

    /// Layout spacing scale — use these for padding and stack gaps.
    enum Spacing {
        /// 4 pt — micro gap between tightly related elements.
        static let xs: CGFloat = 4
        /// 8 pt — small inset or gap.
        static let sm: CGFloat = 8
        /// 16 pt — standard content padding.
        static let md: CGFloat = 16
        /// 24 pt — section-level spacing.
        static let lg: CGFloat = 24
        /// 32 pt — large section gap or hero padding.
        static let xl: CGFloat = 32
    }

    // MARK: - Corner radius

    /// Corner-radius scale for cards, buttons, and surfaces.
    enum Corner {
        /// 8 pt — small buttons and chips.
        static let sm: CGFloat = 8
        /// 12 pt — compact cards and input fields.
        static let md: CGFloat = 12
        /// 14 pt — tool-batch and suggestion chip pills.
        static let pill: CGFloat = 14
        /// 16 pt — standard cards and glass surfaces.
        static let lg: CGFloat = 16
        /// 18 pt — chat message bubbles (agent and feedback).
        static let bubble: CGFloat = 18
        /// 24 pt — large hero cards and bottom sheets.
        static let xl: CGFloat = 24
    }

    // MARK: - Layout sizes

    /// Fixed-size tokens for icons, avatars, circular buttons, shared
    /// chat-bubble geometry, and canonical List row insets.
    ///
    /// Use these instead of hardcoding `frame(width:height:)` values or
    /// repeating `EdgeInsets(top:leading:bottom:trailing:)` literals.
    enum Layout {
        /// 36 pt — small circular avatar or icon-only button tap target.
        static let iconSm: CGFloat = 36
        /// 64 pt — medium profile avatar.
        static let iconLg: CGFloat = 64
        /// 60 pt — minimum leading/trailing spacer that pushes a chat bubble
        /// to the opposite edge (used in FeedbackBubble and
        /// FeedbackThreadDetailView for the image bubble row).
        static let bubbleSpacer: CGFloat = 60
        /// 2 pt — vertical padding between adjacent bubble rows in a thread
        /// (used in FeedbackBubble and FeedbackThreadDetailView).
        static let bubbleRowSpacing: CGFloat = 2

        // MARK: List row insets

        /// Standard card-row insets (xs vertical, md horizontal) — use for
        /// compact card cells such as key-info cards in settings screens.
        ///
        /// `EdgeInsets(top: 4, leading: 16, bottom: 4, trailing: 16)`
        static let cardRowInsetsXS: EdgeInsets = EdgeInsets(
            top: Spacing.xs,
            leading: Spacing.md,
            bottom: Spacing.xs,
            trailing: Spacing.md
        )

        /// Standard card-row insets (sm vertical, md horizontal) — use for
        /// card cells, segmented controls, and profile headers inside Lists.
        ///
        /// `EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16)`
        static let cardRowInsetsSM: EdgeInsets = EdgeInsets(
            top: Spacing.sm,
            leading: Spacing.md,
            bottom: Spacing.sm,
            trailing: Spacing.md
        )
    }
}
