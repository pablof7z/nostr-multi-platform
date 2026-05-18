import SwiftUI

// OWNER: Phase-2 Agent D — Activity (polished "Coming in M7" surface).

struct NotificationsView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        ScrollView {
            VStack(spacing: ChirpSpace.xxl) {
                // Spacer to center illustration
                Spacer(minLength: 60)
                illustrationBlock
                typeGrid
                Spacer(minLength: ChirpSpace.xl)
            }
            .padding(.horizontal, ChirpSpace.l)
        }
        .background(Color(.systemBackground))
        .navigationTitle("Activity")
        .navigationBarTitleDisplayMode(.large)
    }

    // ── Central illustration ──────────────────────────────────────────────

    private var illustrationBlock: some View {
        VStack(spacing: ChirpSpace.l) {
            // Layered bell icon with soft glow rings
            ZStack {
                Circle()
                    .fill(ChirpColor.accentSoft)
                    .frame(width: 120, height: 120)
                Circle()
                    .fill(ChirpColor.accentSoft.opacity(0.5))
                    .frame(width: 90, height: 90)
                Circle()
                    .fill(ChirpColor.accentSoft)
                    .frame(width: 64, height: 64)
                Image(systemName: "bell.badge")
                    .font(.system(size: 30, weight: .light))
                    .foregroundStyle(ChirpColor.accent)
                    .symbolRenderingMode(.hierarchical)
            }

            VStack(spacing: ChirpSpace.s) {
                Text("Your Activity Feed")
                    .font(ChirpFont.title)
                    .foregroundStyle(ChirpColor.textPrimary)
                    .multilineTextAlignment(.center)

                Text("Mentions, reactions, reposts and zaps\nwill all live here in one stream.")
                    .font(ChirpFont.callout)
                    .foregroundStyle(ChirpColor.textSecondary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }

            // Version tag pill
            HStack(spacing: ChirpSpace.xs) {
                Image(systemName: "clock")
                    .font(.system(size: 11, weight: .semibold))
                Text("Coming in Chirp v1 M7")
                    .font(.system(.caption, design: .rounded).weight(.semibold))
            }
            .foregroundStyle(ChirpColor.accent)
            .padding(.horizontal, ChirpSpace.m)
            .padding(.vertical, ChirpSpace.xs)
            .background(ChirpColor.accentSoft, in: Capsule())
            .overlay(Capsule().strokeBorder(ChirpColor.accent.opacity(0.25)))
        }
        .frame(maxWidth: .infinity)
    }

    // ── Activity type preview grid ────────────────────────────────────────

    private var typeGrid: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "What to expect")

            LazyVGrid(
                columns: [GridItem(.flexible()), GridItem(.flexible())],
                spacing: ChirpSpace.m
            ) {
                ActivityTypeCard(
                    icon: "at",
                    label: "Mentions",
                    description: "When others tag you in a note",
                    color: ChirpColor.accent
                )
                ActivityTypeCard(
                    icon: "heart.fill",
                    label: "Reactions",
                    description: "Likes and custom emoji reactions",
                    color: ChirpColor.like
                )
                ActivityTypeCard(
                    icon: "arrow.2.squarepath",
                    label: "Reposts",
                    description: "Your notes boosted by followers",
                    color: ChirpColor.positive
                )
                ActivityTypeCard(
                    icon: "bolt.fill",
                    label: "Zaps",
                    description: "Lightning payments sent to you",
                    color: ChirpColor.zap
                )
            }
        }
    }
}

// ── Activity type card ─────────────────────────────────────────────────────

private struct ActivityTypeCard: View {
    let icon: String
    let label: String
    let description: String
    let color: Color

    var body: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: ChirpSpace.s) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(color.opacity(0.14))
                        .frame(width: 36, height: 36)
                    Image(systemName: icon)
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(color)
                }
                Text(label)
                    .font(ChirpFont.callout.weight(.semibold))
                    .foregroundStyle(ChirpColor.textPrimary)
                Text(description)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .lineLimit(3)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
