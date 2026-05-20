import SwiftUI

struct NotificationsView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        List {
            Section {
                illustrationBlock
            }

            Section("What to expect") {
                ActivityTypeRow(
                    icon: "at",
                    label: "Mentions",
                    description: "When others tag you in a note"
                )
                ActivityTypeRow(
                    icon: "heart.fill",
                    label: "Reactions",
                    description: "Likes and custom emoji reactions"
                )
                ActivityTypeRow(
                    icon: "arrow.2.squarepath",
                    label: "Reposts",
                    description: "Your notes boosted by followers"
                )
                ActivityTypeRow(
                    icon: "bolt.fill",
                    label: "Zaps",
                    description: "Lightning payments sent to you"
                )
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
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
        .padding(.vertical, 32)
    }
}

private struct ActivityTypeRow: View {
    let icon: String
    let label: String
    let description: String

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(.tint)
                .frame(width: 22)
            VStack(alignment: .leading, spacing: 3) {
                Text(label)
                    .font(.callout.weight(.semibold))
                Text(description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}
