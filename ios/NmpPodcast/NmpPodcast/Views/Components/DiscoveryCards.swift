import SwiftUI

// MARK: - Recommendation Card

struct RecommendationCard: View {
    let podcast: PodcastIndexPodcast
    let reason: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            CachedAsyncImage(url: podcast.artworkURL) {
                RoundedRectangle(cornerRadius: 12)
                    .fill(Color.gray.opacity(0.2))
            }
            .aspectRatio(1, contentMode: .fill)
            .frame(width: 160, height: 160)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .shadow(color: .black.opacity(0.1), radius: 4, y: 2)

            VStack(alignment: .leading, spacing: 4) {
                Text(podcast.title)
                    .font(.subheadline)
                    .fontWeight(.semibold)
                    .lineLimit(2)
                    .frame(width: 160, alignment: .leading)

                if let reason {
                    Text(reason)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .frame(width: 160, alignment: .leading)
                } else if let author = podcast.author {
                    Text(author)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .frame(width: 160, alignment: .leading)
                }
            }
        }
    }
}

// MARK: - Featured Trending Card

struct FeaturedTrendingCard: View {
    let podcast: PodcastIndexPodcast
    let rank: Int?

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            CachedAsyncImage(url: podcast.artworkURL) {
                Rectangle()
                    .fill(Color.gray.opacity(0.2))
            }
            .aspectRatio(16/9, contentMode: .fill)
            .frame(height: 200)
            .clipShape(RoundedRectangle(cornerRadius: 16))

            // Gradient overlay
            LinearGradient(
                colors: [.clear, .black.opacity(0.7)],
                startPoint: .top,
                endPoint: .bottom
            )
            .clipShape(RoundedRectangle(cornerRadius: 16))

            // Content overlay
            VStack(alignment: .leading, spacing: 4) {
                if let rank {
                    Text("#\(rank) Trending")
                        .font(.caption)
                        .fontWeight(.bold)
                        .foregroundStyle(.white.opacity(0.8))
                }

                Text(podcast.title)
                    .font(.title3)
                    .fontWeight(.bold)
                    .foregroundStyle(.white)
                    .lineLimit(2)

                if let author = podcast.author {
                    Text(author)
                        .font(.subheadline)
                        .foregroundStyle(.white.opacity(0.8))
                        .lineLimit(1)
                }
            }
            .padding()
        }
        .shadow(color: .black.opacity(0.15), radius: 8, y: 4)
    }
}

// MARK: - Trending Grid Card

struct TrendingGridCard: View {
    let podcast: PodcastIndexPodcast
    let rank: Int

    var body: some View {
        ZStack(alignment: .topLeading) {
            CachedAsyncImage(url: podcast.artworkURL) {
                Rectangle()
                    .fill(Color.gray.opacity(0.2))
            }
            .aspectRatio(1, contentMode: .fill)
            .clipShape(RoundedRectangle(cornerRadius: 12))

            // Rank badge
            Text("\(rank)")
                .font(.caption)
                .fontWeight(.bold)
                .foregroundStyle(.white)
                .frame(width: 24, height: 24)
                .background(Circle().fill(.black.opacity(0.6)))
                .padding(8)
        }
        .shadow(color: .black.opacity(0.1), radius: 4, y: 2)
    }
}

// MARK: - Category Card

struct CategoryCard: View {
    let category: PodcastIndexCategory

    private var gradient: LinearGradient {
        let colors = categoryColors[category.id % categoryColors.count]
        return LinearGradient(
            colors: colors,
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
    }

    private var icon: String {
        categoryIcons[category.name] ?? "mic.fill"
    }

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            gradient

            // Icon in top right
            Image(systemName: icon)
                .font(.system(size: 40))
                .foregroundStyle(.white.opacity(0.3))
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
                .padding(12)

            // Category name
            Text(category.name)
                .font(.headline)
                .fontWeight(.bold)
                .foregroundStyle(.white)
                .padding(12)
        }
        .frame(height: 100)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .shadow(color: .black.opacity(0.1), radius: 4, y: 2)
    }
}

// MARK: - Topic Pill

struct TopicPill: View {
    let topic: String
    let isSelected: Bool

    var body: some View {
        Text(topic)
            .font(.subheadline)
            .fontWeight(.medium)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(isSelected ? Color.blue : Color(.systemGray5))
            .foregroundStyle(isSelected ? .white : .primary)
            .clipShape(Capsule())
    }
}

// MARK: - Section Header

struct DiscoverySectionHeader: View {
    let title: String
    let subtitle: String?
    let action: (() -> Void)?

    init(title: String, subtitle: String? = nil, action: (() -> Void)? = nil) {
        self.title = title
        self.subtitle = subtitle
        self.action = action
    }

    var body: some View {
        HStack(alignment: .bottom) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.title2)
                    .fontWeight(.bold)

                if let subtitle {
                    Text(subtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer()

            if let action {
                Button("See All", action: action)
                    .font(.subheadline)
                    .fontWeight(.medium)
            }
        }
        .padding(.horizontal)
    }
}

// MARK: - Category Colors & Icons

private let categoryColors: [[Color]] = [
    [Color(red: 0.98, green: 0.36, blue: 0.35), Color(red: 0.89, green: 0.22, blue: 0.40)], // Red-Pink
    [Color(red: 0.36, green: 0.67, blue: 0.93), Color(red: 0.25, green: 0.47, blue: 0.85)], // Blue
    [Color(red: 0.55, green: 0.78, blue: 0.25), Color(red: 0.35, green: 0.60, blue: 0.18)], // Green
    [Color(red: 0.95, green: 0.55, blue: 0.25), Color(red: 0.88, green: 0.35, blue: 0.22)], // Orange
    [Color(red: 0.65, green: 0.45, blue: 0.90), Color(red: 0.50, green: 0.30, blue: 0.80)], // Purple
    [Color(red: 0.25, green: 0.78, blue: 0.75), Color(red: 0.15, green: 0.55, blue: 0.60)], // Teal
    [Color(red: 0.95, green: 0.75, blue: 0.25), Color(red: 0.90, green: 0.55, blue: 0.20)], // Yellow-Orange
    [Color(red: 0.85, green: 0.35, blue: 0.65), Color(red: 0.70, green: 0.20, blue: 0.50)], // Magenta
]

private let categoryIcons: [String: String] = [
    "Arts": "paintpalette.fill",
    "Business": "briefcase.fill",
    "Comedy": "face.smiling.fill",
    "Education": "book.fill",
    "Fiction": "text.book.closed.fill",
    "Government": "building.columns.fill",
    "Health & Fitness": "heart.fill",
    "History": "clock.fill",
    "Kids & Family": "figure.2.and.child.holdinghands",
    "Leisure": "gamecontroller.fill",
    "Music": "music.note",
    "News": "newspaper.fill",
    "Religion & Spirituality": "sparkles",
    "Science": "atom",
    "Society & Culture": "person.3.fill",
    "Sports": "sportscourt.fill",
    "Technology": "cpu.fill",
    "True Crime": "magnifyingglass",
    "TV & Film": "tv.fill"
]

// MARK: - Previews

#Preview("Recommendation Card") {
    RecommendationCard(
        podcast: PodcastIndexPodcast(
            id: 1,
            title: "The Daily",
            url: "https://example.com",
            description: "Daily news",
            author: "The New York Times",
            image: nil,
            categories: nil,
            newestItemPublishTime: nil
        ),
        reason: "Episodes about AI ethics"
    )
    .padding()
}

#Preview("Featured Card") {
    FeaturedTrendingCard(
        podcast: PodcastIndexPodcast(
            id: 1,
            title: "Huberman Lab",
            url: "https://example.com",
            description: "Science",
            author: "Dr. Andrew Huberman",
            image: nil,
            categories: nil,
            newestItemPublishTime: nil
        ),
        rank: 1
    )
    .padding()
}

#Preview("Category Card") {
    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
        ForEach(PodcastIndexCategory.all.prefix(6)) { category in
            CategoryCard(category: category)
        }
    }
    .padding()
}
