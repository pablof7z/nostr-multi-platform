import SwiftUI
// OWNER: Phase-2 Agent D (Activity — polished stub; reverse-index views
// are M7+, present a finished-looking "coming soon" surface).
struct NotificationsView: View {
    @EnvironmentObject private var model: KernelModel
    var body: some View {
        ChirpPlaceholder(systemImage: "bell.fill", title: "Activity",
            subtitle: "Agent D builds this.")
            .navigationTitle("Activity")
    }
}
