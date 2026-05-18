import SwiftUI
import SwiftData

struct PodcastDetailSheet: View {
    let podcast: PodcastIndexPodcast
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext
    @Query private var existingPodcasts: [Podcast]
    @State private var isSubscribing = false
    @State private var errorMessage: String?

    private let podcastService = ServiceContainer.shared.podcastService

    private var isAlreadySubscribed: Bool {
        guard let feedURL = podcast.feedURL else { return false }
        return existingPodcasts.contains { $0.feedURL == feedURL }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 20) {
                    // Artwork
                    CachedAsyncImage(url: podcast.artworkURL) {
                        Rectangle()
                            .fill(Color.gray.opacity(0.3))
                    }
                    .aspectRatio(contentMode: .fill)
                    .frame(width: 200, height: 200)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .shadow(radius: 8)

                    // Title and Author
                    VStack(spacing: 8) {
                        Text(podcast.title)
                            .font(.title2)
                            .fontWeight(.bold)
                            .multilineTextAlignment(.center)

                        if let author = podcast.author {
                            Text(author)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                    }

                    // Subscribe Button
                    if isAlreadySubscribed {
                        Label("Already Subscribed", systemImage: "checkmark.circle.fill")
                            .font(.headline)
                            .foregroundStyle(.green)
                            .padding()
                    } else {
                        Button {
                            subscribe()
                        } label: {
                            if isSubscribing {
                                ProgressView()
                                    .progressViewStyle(.circular)
                                    .tint(.white)
                            } else {
                                Text("Subscribe")
                                    .fontWeight(.semibold)
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .buttonBorderShape(.capsule)
                        .controlSize(.large)
                        .disabled(isSubscribing)
                    }

                    // Error Message
                    if let error = errorMessage {
                        Text(error)
                            .font(.caption)
                            .foregroundStyle(.red)
                            .multilineTextAlignment(.center)
                            .padding(.horizontal)
                    }

                    // Description
                    if let description = podcast.description, !description.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("About")
                                .font(.headline)

                            Text(description)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal)
                    }

                    // Categories
                    if let categories = podcast.categories, !categories.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Categories")
                                .font(.headline)

                            ScrollView(.horizontal, showsIndicators: false) {
                                HStack(spacing: 8) {
                                    ForEach(Array(categories.values.sorted()), id: \.self) { category in
                                        Text(category)
                                            .font(.caption)
                                            .padding(.horizontal, 12)
                                            .padding(.vertical, 6)
                                            .background(Color.secondary.opacity(0.2))
                                            .clipShape(Capsule())
                                    }
                                }
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal)
                    }
                }
                .padding(.vertical)
            }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                }
            }
        }
    }

    private func subscribe() {
        guard let feedURL = podcast.feedURL else {
            errorMessage = "Invalid podcast feed URL"
            return
        }

        isSubscribing = true
        errorMessage = nil

        Task {
            do {
                let subscribedPodcast = try await podcastService.fetchFeed(url: feedURL)

                await MainActor.run {
                    modelContext.insert(subscribedPodcast)
                    isSubscribing = false
                    dismiss()
                }
            } catch {
                await MainActor.run {
                    errorMessage = error.localizedDescription
                    isSubscribing = false
                }
            }
        }
    }
}

#Preview {
    PodcastDetailSheet(
        podcast: PodcastIndexPodcast(
            id: 1,
            title: "Sample Podcast",
            url: "https://example.com/feed.rss",
            description: "This is a sample podcast description that shows how the detail view looks with content.",
            author: "Sample Author",
            image: nil,
            categories: ["1": "Technology", "2": "Science"],
            newestItemPublishTime: nil
        )
    )
    .modelContainer(for: [Podcast.self], inMemory: true)
}
