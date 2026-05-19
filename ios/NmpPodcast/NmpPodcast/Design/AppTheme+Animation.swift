import SwiftUI

extension AppTheme {

    // MARK: - Animation presets

    /// Pre-tuned SwiftUI animation curves for consistent motion.
    enum Animation {
        /// Default spring — most transitions and reveals.
        static let spring = SwiftUI.Animation.spring(duration: 0.35, bounce: 0.15)
        /// Fast spring — quick feedback interactions.
        static let springFast = SwiftUI.Animation.spring(duration: 0.22, bounce: 0.12)
        /// Bouncy spring — playful entrances.
        static let springBouncy = SwiftUI.Animation.spring(duration: 0.45, bounce: 0.3)
        /// Ease-out — elements sliding into a resting position.
        static let easeOut = SwiftUI.Animation.easeOut(duration: 0.25)
        /// Ease-in — elements leaving the screen.
        static let easeIn = SwiftUI.Animation.easeIn(duration: 0.2)
        /// Ease-in-out — looping UI elements such as typing-indicator dots.
        static let easeInOut = SwiftUI.Animation.easeInOut(duration: 0.3)
    }

    // MARK: - Timing

    /// Duration constants for Task.sleep-based UI feedback and animation delays.
    ///
    /// Use these instead of hardcoding raw `.seconds()` / `.milliseconds()` values
    /// so all copy-feedback, completion-animation, and typing-indicator delays stay in sync.
    enum Timing {
        /// 1.5 s — standard "Copied!" chip display time across agent/identity views.
        static let copyFeedback: Duration = .seconds(1.5)
        /// 200 ms — delay before committing item completion after the springFast
        /// exit animation fires (~220 ms total). Keeps the row visually gone before
        /// the data model updates and SwiftUI removes it from the list.
        static let completionExit: Duration = .milliseconds(200)
        /// 350 ms — typing-indicator dot phase cycle step.
        static let typingDotStep: Duration = .milliseconds(350)
        /// 600 ms — simulated publish latency for a new feedback thread.
        static let feedbackPublishDelay: Duration = .milliseconds(600)
        /// 300 ms — simulated reply latency for a feedback thread reply.
        static let feedbackReplyDelay: Duration = .milliseconds(300)
        /// 120 ms — inter-beat gap for two-beat haptic patterns (complete/reopen).
        static let hapticTwoBeat: Duration = .milliseconds(120)
        /// 100 ms — inter-beat gap for undo haptic pattern.
        static let hapticUndo: Duration = .milliseconds(100)
        /// 80 ms — inter-beat gap for bulk-action haptic pattern.
        static let hapticBulk: Duration = .milliseconds(80)
    }
}
