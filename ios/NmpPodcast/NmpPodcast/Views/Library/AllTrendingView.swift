import SwiftUI

// MARK: - All Trending View

struct AllTrendingView: View {
    @Environment(\.dismiss) private var dismiss
    let podcasts: [PodcastIndexPodcast]
    @State private var selectedPodcast: PodcastIndexPodcast?

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(Array(podcasts.enumerated()), id: \.element.id) { index, podcast in
                        HStack(spacing: 12) {
                            Text("\(index + 1)")
                                .font(.headline)
                                .foregroundStyle(.secondary)
                                .frame(width: 30)

                            CachedAsyncImage(url: podcast.artworkURL) {
                                RoundedRectangle(cornerRadius: 8)
                                    .fill(Color.gray.opacity(0.2))
                            }
                            .aspectRatio(1, contentMode: .fill)
                            .frame(width: 60, height: 60)
                            .clipShape(RoundedRectangle(cornerRadius: 8))

                            VStack(alignment: .leading, spacing: 4) {
                                Text(podcast.title)
                                    .font(.subheadline)
                                    .fontWeight(.medium)
                                    .lineLimit(2)

                                if let author = podcast.author {
                                    Text(author)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                }
                            }

                            Spacer()
                        }
                        .padding(.horizontal)
                        .padding(.vertical, 10)
                        .contentShape(Rectangle())
                        .onTapGesture {
                            selectedPodcast = podcast
                        }

                        if index < podcasts.count - 1 {
                            Divider()
                                .padding(.leading, 102)
                        }
                    }
                }
            }
            .navigationTitle("Trending")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                }
            }
            .sheet(item: $selectedPodcast) { podcast in
                PodcastDetailSheet(podcast: podcast)
            }
        }
    }
}
