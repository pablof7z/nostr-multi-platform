import SwiftUI

// MARK: - All Categories View

struct AllCategoriesView: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
                    ForEach(PodcastIndexCategory.all) { category in
                        NavigationLink {
                            CategoryDetailView(category: category)
                        } label: {
                            CategoryCard(category: category)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding()
            }
            .navigationTitle("Categories")
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
}

// MARK: - Category Detail View

struct CategoryDetailView: View {
    let category: PodcastIndexCategory
    @State private var podcasts: [PodcastIndexPodcast] = []
    @State private var isLoading = false
    @State private var selectedPodcast: PodcastIndexPodcast?

    private let podcastIndexService = ServiceContainer.shared.podcastIndexService

    var body: some View {
        ScrollView {
            if isLoading {
                ProgressView()
                    .frame(maxWidth: .infinity)
                    .padding(.top, 40)
            } else if podcasts.isEmpty {
                VStack(spacing: 12) {
                    Image(systemName: "mic.slash")
                        .font(.largeTitle)
                        .foregroundStyle(.secondary)
                    Text("No podcasts found")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.top, 40)
            } else {
                LazyVStack(alignment: .leading, spacing: 16) {
                    // Hero section with top 3
                    if podcasts.count >= 3 {
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 16) {
                                ForEach(podcasts.prefix(3)) { podcast in
                                    RecommendationCard(podcast: podcast, reason: nil)
                                        .onTapGesture {
                                            selectedPodcast = podcast
                                        }
                                }
                            }
                            .padding(.horizontal)
                        }
                    }

                    // Rest of podcasts
                    VStack(alignment: .leading, spacing: 0) {
                        Text("All in \(category.name)")
                            .font(.headline)
                            .padding(.horizontal)
                            .padding(.bottom, 12)

                        ForEach(Array(podcasts.dropFirst(3).enumerated()), id: \.element.id) { index, podcast in
                            PodcastSearchRow(podcast: podcast)
                                .contentShape(Rectangle())
                                .onTapGesture {
                                    selectedPodcast = podcast
                                }

                            if index < podcasts.count - 4 {
                                Divider()
                                    .padding(.leading, 84)
                            }
                        }
                    }
                }
                .padding(.top)
            }
        }
        .navigationTitle(category.name)
        .navigationBarTitleDisplayMode(.large)
        .sheet(item: $selectedPodcast) { podcast in
            PodcastDetailSheet(podcast: podcast)
        }
        .task {
            await loadPodcasts()
        }
    }

    private func loadPodcasts() async {
        isLoading = true
        do {
            let results = try await podcastIndexService.podcastsByCategory(category.name, limit: 50)
            await MainActor.run {
                podcasts = results
                isLoading = false
            }
        } catch {
            await MainActor.run {
                isLoading = false
            }
        }
    }
}
