import SwiftUI

extension AppTheme {

    // MARK: - Brand colors

    enum Brand {
        static let elevenLabsTint = SwiftUI.Color(red: 0, green: 0.78, blue: 0.62)
    }

    // MARK: - Semantic tints

    enum Tint {
        static let error = SwiftUI.Color.red
        static let errorOnDark = SwiftUI.Color(red: 1.0, green: 0.7, blue: 0.7)
        static let success = SwiftUI.Color.green
        static let warning = SwiftUI.Color.orange
        static let onboardingChipAI = SwiftUI.Color(red: 0.80, green: 0.70, blue: 1.0)
        static let onboardingChipFriends = SwiftUI.Color(red: 0.60, green: 0.88, blue: 1.0)
        static let onboardingChipFeedback = SwiftUI.Color(red: 0.70, green: 1.0, blue: 0.85)
        static let agentSurface = SwiftUI.Color.indigo
        static let hairline = SwiftUI.Color.secondary.opacity(0.18)
        static let surfaceMuted = SwiftUI.Color.secondary.opacity(0.08)
        static let surfaceFaint = SwiftUI.Color.secondary.opacity(0.04)
        static let dimmed = SwiftUI.Color.secondary.opacity(0.4)
        static let placeholder = SwiftUI.Color.secondary.opacity(0.15)
        /// Editorial amber used by wiki citations, threading mentions,
        /// and contradiction markers. Five sites previously inlined the
        /// same RGB literal — `Color(red: 0.72, green: 0.45, blue: 0.10)` —
        /// or declared their own `static let amber`. Consolidated here so
        /// any future tone tweak lands in one place.
        static let editorialAmber = SwiftUI.Color(red: 0.72, green: 0.45, blue: 0.10)

        // MARK: Evidence / confidence levels (Wiki + Threading)

        /// Multi-source / high-confidence evidence — solid green left rule.
        static let evidenceHigh = SwiftUI.Color(red: 0.18, green: 0.55, blue: 0.34)
        /// Single-source / medium-confidence evidence — amber dotted left rule.
        static let evidenceMedium = SwiftUI.Color(red: 0.78, green: 0.55, blue: 0.10)
        /// Uncorroborated / low-confidence evidence — red dotted left rule.
        static let evidenceLow = SwiftUI.Color(red: 0.78, green: 0.18, blue: 0.30)
        /// Contradiction / contradiction-seam tint in threading surfaces.
        static let threadingContradiction = SwiftUI.Color(red: 0.85, green: 0.64, blue: 0.25)

        // MARK: Voice conversation state

        /// Voice listening / barge-in state — sky blue.
        static let voiceListening = SwiftUI.Color(red: 0.45, green: 0.78, blue: 1.0)
        /// Voice thinking state — lavender.
        static let voiceThinking = SwiftUI.Color(red: 0.62, green: 0.45, blue: 1.0)
        /// Voice speaking state — mint.
        static let voiceSpeaking = SwiftUI.Color(red: 0.36, green: 0.85, blue: 0.78)
    }

    // MARK: - Gradients

    enum Gradients {
        static let agentAccent = LinearGradient(
            colors: [
                Color(red: 0.36, green: 0.20, blue: 0.84),
                Color(red: 0.14, green: 0.45, blue: 0.92),
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )

        static let onboardingNebula = LinearGradient(
            colors: [
                Color(red: 0.05, green: 0.04, blue: 0.20),
                Color(red: 0.18, green: 0.10, blue: 0.42),
                Color(red: 0.10, green: 0.32, blue: 0.66),
                Color(red: 0.04, green: 0.55, blue: 0.74)
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )

        static let onboardingSparkle = LinearGradient(
            colors: [.white, Color(red: 0.85, green: 0.92, blue: 1.0)],
            startPoint: .top,
            endPoint: .bottom
        )

        static let onboardingSuccess = LinearGradient(
            colors: [.white, Color(red: 0.7, green: 1.0, blue: 0.85)],
            startPoint: .top,
            endPoint: .bottom
        )

        static let agentChatBackground = LinearGradient(
            colors: [
                Color(.systemBackground),
                AppTheme.Tint.agentSurface.opacity(0.05),
                Color.blue.opacity(0.04),
            ],
            startPoint: .top,
            endPoint: .bottom
        )
    }
}
