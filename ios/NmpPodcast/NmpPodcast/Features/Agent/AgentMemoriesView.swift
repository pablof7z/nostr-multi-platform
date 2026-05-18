import SwiftUI

// MARK: - AgentMemoriesView
//
// T-podcast-gap-002: Stub for Spotlight deep-link destination.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Settings/Agent/AgentMemoriesView.swift

struct AgentMemoriesView: View {
    var spotlightTargetID: UUID? = nil

    var body: some View {
        ContentUnavailableView(
            "Agent Memory",
            systemImage: "brain",
            description: Text("Agent memory loads once the kernel exposes the agent interface (T-podcast-gap-002).")
        )
    }
}
