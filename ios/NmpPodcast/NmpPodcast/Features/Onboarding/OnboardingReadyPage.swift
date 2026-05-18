import SwiftUI

// MARK: - Ready

/// The final onboarding page. Confirms setup is complete and previews the
/// four core capability areas the user can explore immediately.
struct OnboardingReadyPage: View {
    @State private var bounceTrigger: Int = 0

    private enum Layout {
        static let medallionSize: CGFloat = 180
        static let medallionIconSize: CGFloat = 100
        static let featureColumns: Int = 2
        static let featureIconSize: CGFloat = 22
        static let featureCornerRadius: CGFloat = AppTheme.Corner.lg
        static let featureVerticalPadding: CGFloat = 14
        static let featureHorizontalPadding: CGFloat = 12
        static let featureGridSpacing: CGFloat = AppTheme.Spacing.sm
    }

    /// The four headline features surfaced to the user on completion.
    private let features: [(icon: String, label: String, color: Color)] = [
        ("antenna.radiowaves.left.and.right", "Subscribe via OPML or feed URL", AppTheme.Tint.onboardingChipAI),
        ("headphones", "Listen with chapters & speed control", .white),
        ("sparkles", "Ask the agent about anything you've heard", AppTheme.Tint.onboardingChipFriends),
        ("book.closed.fill", "Auto-generated wikis from transcripts", AppTheme.Tint.onboardingChipFeedback),
    ]

    var body: some View {
        VStack(spacing: AppTheme.Spacing.lg) {
            Spacer()

            checkmarkMedallion

            VStack(spacing: AppTheme.Spacing.sm) {
                Text("You're all set")
                    .font(AppTheme.Typography.largeTitle)
                    .foregroundStyle(.white)

                Text("Here's what you can do right away.")
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.white.opacity(0.85))
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, AppTheme.Spacing.md)
            }

            featureGrid

            Spacer()
            Spacer()
        }
    }

    // MARK: - Checkmark medallion

    private var checkmarkMedallion: some View {
        ZStack {
            Circle()
                .fill(.white.opacity(0.15))
                .frame(width: Layout.medallionSize, height: Layout.medallionSize)
                .glassEffect(.regular, in: .circle)
                .overlay(Circle().strokeBorder(.white.opacity(0.35), lineWidth: 1))
                .appShadow(AppTheme.Shadow.lifted)

            Image(systemName: "checkmark.seal.fill")
                .font(.system(size: Layout.medallionIconSize, weight: .bold))
                .foregroundStyle(AppTheme.Gradients.onboardingSuccess)
                .symbolEffect(.bounce, options: .repeat(3), value: bounceTrigger)
                .appShadow(AppTheme.Shadow.onboardingIconGlow)
        }
        .onAppear {
            bounceTrigger += 1
        }
    }

    // MARK: - Feature grid

    private var featureGrid: some View {
        let columns = Array(repeating: GridItem(.flexible(), spacing: Layout.featureGridSpacing), count: Layout.featureColumns)
        return LazyVGrid(columns: columns, spacing: Layout.featureGridSpacing) {
            ForEach(features, id: \.label) { feature in
                featureChip(icon: feature.icon, label: feature.label, color: feature.color)
            }
        }
        .padding(.horizontal, AppTheme.Spacing.md)
    }

    private func featureChip(icon: String, label: String, color: Color) -> some View {
        HStack(alignment: .top, spacing: AppTheme.Spacing.sm) {
            Image(systemName: icon)
                .font(.system(size: Layout.featureIconSize, weight: .semibold))
                .foregroundStyle(color)
                .frame(width: Layout.featureIconSize + 2, alignment: .center)
            Text(label)
                .font(AppTheme.Typography.callout.weight(.medium))
                .foregroundStyle(.white.opacity(0.90))
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Layout.featureHorizontalPadding)
        .padding(.vertical, Layout.featureVerticalPadding)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassEffect(.regular, in: .rect(cornerRadius: Layout.featureCornerRadius))
        .overlay(
            RoundedRectangle(cornerRadius: Layout.featureCornerRadius, style: .continuous)
                .strokeBorder(.white.opacity(0.20), lineWidth: 1)
        )
    }
}
