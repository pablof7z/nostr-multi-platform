import SwiftUI
import UIKit

extension Color {
    /// Terracotta accent used for clip ranges, primary CTAs, and highlighted
    /// segments. Matches the webapp's `--color-highlighter-accent`. Dark
    /// variant is slightly brighter so it stays legible on the dark paper.
    static let highlighterAccent = Color(uiColor: UIColor { trait in
        trait.userInterfaceStyle == .dark
            ? UIColor(red: 0.88, green: 0.60, blue: 0.46, alpha: 1)
            : UIColor(red: 0.77, green: 0.49, blue: 0.37, alpha: 1)
    })

    /// Page background — warm ivory in light, deep warm ink in dark.
    static let highlighterPaper = Color(uiColor: UIColor { trait in
        trait.userInterfaceStyle == .dark
            ? UIColor(red: 0.082, green: 0.078, blue: 0.067, alpha: 1)
            : UIColor(red: 0.98, green: 0.98, blue: 0.97, alpha: 1)
    })

    /// Module surface — a warmer, slightly darker paper used to wrap a
    /// grouped feed item (resource header + highlight content) so the
    /// section reads as one coherent module against the page paper.
    static let highlighterPaperTint = Color(uiColor: UIColor { trait in
        trait.userInterfaceStyle == .dark
            ? UIColor(red: 0.115, green: 0.108, blue: 0.092, alpha: 1)
            : UIColor(red: 0.945, green: 0.925, blue: 0.878, alpha: 1)
    })

    /// Primary body type.
    static let highlighterInkStrong = Color(uiColor: UIColor { trait in
        trait.userInterfaceStyle == .dark
            ? UIColor(red: 0.957, green: 0.945, blue: 0.918, alpha: 1)
            : UIColor(red: 0.082, green: 0.075, blue: 0.059, alpha: 1)
    })

    /// Muted metadata / secondary type.
    static let highlighterInkMuted = Color(uiColor: UIColor { trait in
        trait.userInterfaceStyle == .dark
            ? UIColor(red: 0.678, green: 0.651, blue: 0.588, alpha: 1)
            : UIColor(red: 0.478, green: 0.455, blue: 0.408, alpha: 1)
    })

    /// Hairlines / dividers / separator rules.
    static let highlighterRule = Color(uiColor: UIColor { trait in
        trait.userInterfaceStyle == .dark
            ? UIColor(red: 0.212, green: 0.200, blue: 0.173, alpha: 1)
            : UIColor(red: 0.898, green: 0.878, blue: 0.816, alpha: 1)
    })

    /// Pale blue tint used behind subtle informational surfaces.
    static let highlighterTintPale = Color(uiColor: UIColor { trait in
        trait.userInterfaceStyle == .dark
            ? UIColor(red: 0.106, green: 0.145, blue: 0.192, alpha: 1)
            : UIColor(red: 0.91, green: 0.955, blue: 0.992, alpha: 1)
    })

    /// Highlighter-stroke underlay used to mark matched text in search
    /// results. Accent at low opacity, scoped to the matched run.
    static let laneArticleHighlightFill = Color(uiColor: UIColor { trait in
        trait.userInterfaceStyle == .dark
            ? UIColor(red: 0.88, green: 0.60, blue: 0.46, alpha: 0.22)
            : UIColor(red: 0.95, green: 0.78, blue: 0.42, alpha: 0.32)
    })
}
