import SwiftUI

// MARK: - AgentChatView
//
// T-podcast-gap-002: Verbatim Podcastr AgentChatView requires full agent
// infrastructure. Stub until kernel exposes agent interface.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Agent/AgentChatView.swift

struct AgentChatView: View {
    let session: AgentChatSession

    var body: some View {
        ContentUnavailableView(
            "Agent",
            systemImage: "sparkles",
            description: Text("AI agent chat loads once the kernel exposes the agent interface (T-podcast-gap-002).")
        )
        .navigationTitle("Agent")
        .navigationBarTitleDisplayMode(.inline)
    }
}
