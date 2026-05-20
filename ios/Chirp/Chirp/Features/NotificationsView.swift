import SwiftUI

struct NotificationsView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        ScrollView {
            VStack(spacing: ChirpSpace.l) {
                illustrationBlock
                typeGrid
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.top, ChirpSpace.m)
            .padding(.bottom, ChirpSpace.xl)
        }
        .chirpScreenBackground()
        .navigationTitle("Activity")
        .navigationBarTitleDisplayMode(.large)
    }

    private var illustrationBlock: some View {
        VStack(spacing: ChirpSpace.m) {
            Image(systemName: "bell.badge")
                .font(.system(size: 28, weight: .semibold))
                .symbolRenderingMode(.hierarchical)

            VStack(spacing: ChirpSpace.s) {
                Text("No activity yet")
                    .font(.title3.weight(.semibold))
                    .multilineTextAlignment(.center)

                Text("Mentions, replies, reactions, and boosts will appear here when the kernel projects them.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(ChirpSpace.xl)
        .chirpGlass(cornerRadius: ChirpSpace.radius)
    }

    private var typeGrid: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Activity types")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .textCase(.uppercase)

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
                    description: "Likes and custom reactions"
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
