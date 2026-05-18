import SwiftUI

/// T156 — Library tab, wired end-to-end to the kernel.
///
/// IMPORTANT — this is NOT yet the verbatim copy from
/// `/Users/pablofernandez/src/podcast/PodcastApp/Views/Library/LibraryView.swift`.
/// The canonical file (preserved on disk at
/// `NmpPodcast/Views/Library/LibraryView.swift`) references SwiftData
/// `@Query`, the `Podcast` `@Model` class, `AudioService`, `ProcessingQueue`
/// — none of which exist in NmpPodcast (M11 forbids them: the kernel owns
/// every byte of state). Restoring pixel parity requires either a Swift
/// `Podcast` shim type that proxies kernel snapshot rows, or the
/// `nmp gen modules` generator promised in
/// `docs/design/podcast-app-rebuild.md` §1.
///
/// Filed as T-podcast-gap-001. Until then, this file renders the same data
/// (sorted alphabetical, `ContentUnavailableView` empty state, settings +
/// add toolbar buttons, navigation row layout) but against `KernelModel`.
struct LibraryView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var showingAdd = false

    var body: some View {
        NavigationStack {
            List {
                if model.library.podcasts.isEmpty {
                    ContentUnavailableView(
                        "No Podcasts",
                        systemImage: "books.vertical",
                        description: Text("Subscribe to podcasts to build your library.")
                    )
                } else {
                    ForEach(sortedPodcasts) { row in
                        PodcastRow(row: row)
                    }
                    .onDelete(perform: deletePodcasts)
                }
            }
            .listStyle(.plain)
            .navigationTitle("Library")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        showingAdd = true
                    } label: {
                        Image(systemName: "plus")
                    }
                    .accessibilityIdentifier("addPodcastButton")
                }
            }
            .sheet(isPresented: $showingAdd) {
                AddPodcastView()
            }
        }
    }

    private var sortedPodcasts: [PodcastRowPayload] {
        model.library.podcasts.sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    private func deletePodcasts(at offsets: IndexSet) {
        for index in offsets {
            let id = sortedPodcasts[index].id
            model.unsubscribe(podcastID: id)
        }
    }
}

private struct PodcastRow: View {
    let row: PodcastRowPayload

    var body: some View {
        HStack(spacing: 12) {
            artwork
                .frame(width: 48, height: 48)
                .clipShape(RoundedRectangle(cornerRadius: 8))

            VStack(alignment: .leading, spacing: 4) {
                Text(row.title)
                    .font(.subheadline)
                    .fontWeight(.medium)

                if !row.author.isEmpty {
                    Text(row.author)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Text("\(row.episodeCount) episodes")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private var artwork: some View {
        if let url = row.artworkURL {
            AsyncImage(url: url) { image in
                image.resizable().aspectRatio(contentMode: .fill)
            } placeholder: {
                placeholder
            }
        } else {
            placeholder
        }
    }

    private var placeholder: some View {
        RoundedRectangle(cornerRadius: 8)
            .fill(Color.gray.opacity(0.2))
    }
}
