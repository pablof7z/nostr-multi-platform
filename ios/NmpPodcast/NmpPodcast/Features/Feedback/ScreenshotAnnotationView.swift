import SwiftUI

// MARK: - ScreenshotAnnotationView
//
// T-podcast-gap-002: Verbatim Podcastr ScreenshotAnnotationView requires
// ShakeFeedbackKit. Stub until that dependency is integrated.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Feedback/ScreenshotAnnotationView.swift

struct ScreenshotAnnotationView: View {
    @ObservedObject var workflow: FeedbackWorkflow

    var body: some View {
        ContentUnavailableView(
            "Feedback",
            systemImage: "bubble.left.and.ellipsis.bubble.right",
            description: Text("Screenshot annotation loads once ShakeFeedbackKit is integrated (T-podcast-gap-002).")
        )
    }
}
