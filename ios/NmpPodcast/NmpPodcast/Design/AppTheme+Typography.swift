import SwiftUI

extension AppTheme {

    // MARK: - Typography

    /// App-wide font definitions using Dynamic Type text styles.
    enum Typography {
        static let largeTitle = SwiftUI.Font.system(.largeTitle, design: .rounded, weight: .bold)
        static let title = SwiftUI.Font.system(.title2, design: .rounded, weight: .semibold)
        static let title3 = SwiftUI.Font.system(.title3, design: .rounded, weight: .semibold)
        static let headline = SwiftUI.Font.system(.headline, design: .rounded, weight: .semibold)
        static let body = SwiftUI.Font.system(.body, design: .default)
        static let subheadline = SwiftUI.Font.system(.subheadline, design: .default)
        static let callout = SwiftUI.Font.system(.callout, design: .default)
        static let caption = SwiftUI.Font.system(.caption, design: .default).weight(.medium)
        static let caption2 = SwiftUI.Font.system(.caption2, design: .default)
        static let mono = SwiftUI.Font.system(.caption2, design: .monospaced)
        static let monoCaption = SwiftUI.Font.system(.caption, design: .monospaced)
        static let monoCallout = SwiftUI.Font.system(.callout, design: .monospaced)
        static let monoSubheadline = SwiftUI.Font.system(.subheadline, design: .monospaced)
        static let monoBody = SwiftUI.Font.system(.body, design: .monospaced)
    }
}
