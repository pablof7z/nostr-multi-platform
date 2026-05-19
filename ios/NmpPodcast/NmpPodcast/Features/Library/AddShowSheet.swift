import SwiftUI

// MARK: - AddShowSheet
//
// T-podcast-gap-003: Verbatim from Podcastr requires PodcastSearchView,
// SubscriptionService, and OPML import — none backed by kernel yet.
// This stub wires directly to KernelModel.subscribe for the URL path.
//
// Copied structure from:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Library/AddShowSheet.swift

struct AddShowSheet: View {
    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var kernelModel: KernelModel
    @State private var feedURL: String = ""
    @State private var title: String = ""
    @State private var author: String = ""

    var body: some View {
        NavigationStack {
            Form {
                Section("Feed URL") {
                    TextField("https://feeds.example.com/podcast", text: $feedURL)
                        .keyboardType(.URL)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                }
                Section("Optional Metadata") {
                    TextField("Title", text: $title)
                    TextField("Author", text: $author)
                }
            }
            .navigationTitle("Add Podcast")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") {
                        let trimmed = feedURL.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !trimmed.isEmpty else { return }
                        kernelModel.subscribe(
                            feedURL: trimmed,
                            title: title.isEmpty ? nil : title,
                            author: author.isEmpty ? nil : author
                        )
                        dismiss()
                    }
                    .disabled(feedURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .accessibilityIdentifier("addPodcastConfirm")
                }
            }
        }
    }
}
