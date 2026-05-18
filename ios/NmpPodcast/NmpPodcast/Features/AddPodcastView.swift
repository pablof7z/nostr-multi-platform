import SwiftUI

/// T156 — Add-podcast sheet, kernel-backed.
///
/// The canonical app's `AddPodcastView` parses the feed via Swift
/// `PodcastService` (RSSParser + URLSession) on the Swift side. M11 forbids
/// that — RSS parsing moves to the `podcast-feeds` Rust crate, and the
/// `SubscribePodcast` action drives the fetch / parse / persist chain on the
/// Rust side. That chain is not yet wired (`podcast-feeds` is a stub crate);
/// for this iteration the user enters a feed URL plus optional title /
/// author, which subscribes a minimal `PodcastRecord` so the Library list
/// populates and the kernel boundary is exercised. The real RSS fetch is
/// filed as T-podcast-gap-003.
struct AddPodcastView: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var feedURL: String = ""
    @State private var title: String = ""
    @State private var author: String = ""

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Feed URL", text: $feedURL)
                        .keyboardType(.URL)
                        .autocapitalization(.none)
                        .disableAutocorrection(true)
                        .accessibilityIdentifier("addPodcastFeedURL")
                } header: {
                    Text("RSS / Atom feed")
                } footer: {
                    Text("Until podcast-feeds parsing lands, enter the title/author manually. The kernel saves the feed URL so a future iteration can refresh metadata in place.")
                }

                Section("Metadata (optional)") {
                    TextField("Show title", text: $title)
                        .accessibilityIdentifier("addPodcastTitle")
                    TextField("Author", text: $author)
                        .accessibilityIdentifier("addPodcastAuthor")
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
                        model.subscribe(
                            feedURL: feedURL,
                            title: title.isEmpty ? nil : title,
                            author: author.isEmpty ? nil : author
                        )
                        dismiss()
                    }
                    .disabled(feedURL.trimmingCharacters(in: .whitespaces).isEmpty)
                    .accessibilityIdentifier("addPodcastConfirm")
                }
            }
        }
    }
}
