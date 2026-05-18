import SwiftUI

// MARK: - AgentNotesView
//
// T-podcast-gap-002: Stub for Spotlight deep-link destination.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Settings/Agent/AgentNotesView.swift

struct AgentNotesView: View {
    var spotlightTargetID: UUID? = nil

    var body: some View {
        ContentUnavailableView(
            "Agent Notes",
            systemImage: "note.text",
            description: Text("Agent notes load once the kernel exposes the agent interface (T-podcast-gap-002).")
        )
    }
}
