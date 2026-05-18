import SwiftUI
// OWNER: Phase-2 Agent C (Compose sheet). Replace whole file.
// Presented as a sheet from HomeFeedView; dispatch model.publishNote.
struct ComposeView: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss
    var replyToID: String? = nil
    var body: some View {
        ChirpPlaceholder(systemImage: "square.and.pencil", title: "Compose",
            subtitle: "Agent C builds this.")
    }
}
