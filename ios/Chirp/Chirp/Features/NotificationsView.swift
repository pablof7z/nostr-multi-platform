import SwiftUI

struct NotificationsView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                Spacer(minLength: 60)
                illustrationBlock
                typeGrid
                Spacer(minLength: 24)
            }
            .padding(.horizontal, 16)
        }
        .chirpScreenBackground()
        .navigationTitle("Activity")
        .navigationBarTitleDisplayMode(.large)
    }

    private var illustrationBlock: some View {
        VStack(spacing: 16) {
            Image(systemName: "bell.badge")
                .font(.system(size: 30))
                .symbolRenderingMode(.hierarchical)

            VStack(spacing: 8) {
                Text("Your Activity Feed")
                    .font(.title2)
                    .multilineTextAlignment(.center)

                Text("Mentions, reactions, reposts and zaps will all live here in one stream.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Text("Coming in Chirp v1 M7")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(ChirpSpace.xl)
        .chirpGlass(cornerRadius: ChirpSpace.radius)
    }

    private var typeGrid: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("What to expect")
                .font(.caption)
                .foregroundStyle(.secondary)

            LazyVGrid(
                columns: [GridItem(.flexible()), GridItem(.flexible())],
                spacing: 12
            ) {
                ActivityTypeCard(
                    icon: "at",
                    label: "Mentions",
                    description: "When others tag you in a note"
                )
                ActivityTypeCard(
                    icon: "heart.fill",
                    label: "Reactions",
                    description: "Likes and custom emoji reactions"
                )
                ActivityTypeCard(
                    icon: "arrow.2.squarepath",
                    label: "Reposts",
                    description: "Your notes boosted by followers"
                )
                ActivityTypeCard(
                    icon: "bolt.fill",
                    label: "Zaps",
                    description: "Lightning payments sent to you"
                )
            }
        }
        .padding(ChirpSpace.l)
        .chirpGlass(cornerRadius: ChirpSpace.radius)
    }
}

private struct ActivityTypeCard: View {
    let icon: String
    let label: String
    let description: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Image(systemName: icon)
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(.tint)
            Text(label)
                .font(.callout.weight(.semibold))
            Text(description)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .lineLimit(3)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
